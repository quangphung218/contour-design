//! Conversations, messages, run event batches and agent sessions
//! (spec 3.3).

use crate::error::{Result, StoreError};
use crate::paths::is_safe_id;
use crate::store::Store;
use od_contracts::{
    Conversation, CreateConversationRequest, Message, MessageRole, RunStatus, SessionMode,
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::Value;

/// One row of `agent_sessions`, keyed by (conversation, agent).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSession {
    pub conversation_id: String,
    pub agent_id: String,
    pub session_id: String,
    pub stable_prompt_hash: Option<String>,
    pub model: Option<String>,
    pub cwd: Option<String>,
    /// The assistant message this session last produced (resume cursor).
    pub last_message_id: Option<String>,
}

/// The two messages a run starts with.
#[derive(Debug, Clone)]
pub struct NewTurn<'a> {
    pub conversation_id: &'a str,
    pub user_message_id: &'a str,
    pub assistant_message_id: &'a str,
    pub content: &'a str,
    pub attachments: &'a [String],
    pub agent_id: &'a str,
    pub run_id: &'a str,
}

fn mode_str(mode: SessionMode) -> &'static str {
    match mode {
        SessionMode::Design => "design",
        SessionMode::Plan => "plan",
        SessionMode::Chat => "chat",
    }
}

fn parse_mode(raw: &str) -> SessionMode {
    match raw {
        "plan" => SessionMode::Plan,
        "chat" => SessionMode::Chat,
        _ => SessionMode::Design,
    }
}

fn parse_status(raw: Option<String>) -> Option<RunStatus> {
    match raw.as_deref()? {
        "running" => Some(RunStatus::Running),
        "succeeded" => Some(RunStatus::Succeeded),
        "failed" => Some(RunStatus::Failed),
        "canceled" => Some(RunStatus::Canceled),
        _ => None,
    }
}

fn conversation_from_row(row: &Row<'_>) -> rusqlite::Result<Conversation> {
    let mode: String = row.get(3)?;
    Ok(Conversation {
        id: row.get(0)?,
        project_id: row.get(1)?,
        title: row.get(2)?,
        session_mode: parse_mode(&mode),
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
    })
}

const CONVERSATION_COLUMNS: &str = "id, project_id, title, session_mode, created_at, updated_at";
const MESSAGE_COLUMNS: &str = "id, conversation_id, role, content, agent_id, run_id, run_status, \
     last_run_event_id, position, events_json, attachments_json, started_at, ended_at, created_at";

fn json_list<T: serde::de::DeserializeOwned>(raw: Option<String>) -> Vec<T> {
    raw.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<Message> {
    let role: String = row.get(2)?;
    let last: Option<i64> = row.get(7)?;
    Ok(Message {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        role: if role == "assistant" {
            MessageRole::Assistant
        } else {
            MessageRole::User
        },
        content: row.get(3)?,
        agent_id: row.get(4)?,
        run_id: row.get(5)?,
        run_status: parse_status(row.get(6)?),
        last_run_event_id: last.and_then(|v| u64::try_from(v).ok()),
        position: row.get(8)?,
        events: json_list(row.get(9)?),
        attachments: json_list(row.get(10)?),
        started_at: row.get(11)?,
        ended_at: row.get(12)?,
        created_at: row.get(13)?,
    })
}

fn get_conversation(conn: &Connection, id: &str) -> Result<Option<Conversation>> {
    Ok(conn
        .query_row(
            &format!("SELECT {CONVERSATION_COLUMNS} FROM conversations WHERE id = ?1"),
            [id],
            conversation_from_row,
        )
        .optional()?)
}

/// Position of `message_id` in `conversation_id`, the last message a fork keeps.
fn fork_position(conn: &Connection, conversation_id: &str, message_id: &str) -> Result<i64> {
    conn.query_row(
        "SELECT position FROM messages WHERE id = ?1 AND conversation_id = ?2",
        params![message_id, conversation_id],
        |r| r.get(0),
    )
    .optional()?
    .ok_or_else(|| StoreError::BadRequest("fork message not found".into()))
}

/// Copies messages up to position `upto` under fresh ids. A turn still
/// running in the source ends up canceled in the copy: its run stays with
/// the source conversation.
fn copy_messages(conn: &Connection, from: &str, to: &str, upto: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO messages (id, conversation_id, role, content, agent_id, run_id, run_status, \
           last_run_event_id, position, events_json, attachments_json, comment_attachments_json, \
           produced_files_json, feedback_json, started_at, ended_at, created_at) \
         SELECT lower(hex(randomblob(16))), ?2, role, content, agent_id, run_id, \
           CASE WHEN run_status = 'running' THEN 'canceled' ELSE run_status END, \
           last_run_event_id, position, events_json, attachments_json, comment_attachments_json, \
           produced_files_json, feedback_json, started_at, ended_at, created_at \
         FROM messages WHERE conversation_id = ?1 AND position <= ?3",
        params![from, to, upto],
    )?;
    Ok(())
}

fn next_position(conn: &Connection, conversation_id: &str) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM messages WHERE conversation_id = ?1",
        [conversation_id],
        |r| r.get(0),
    )?)
}

fn touch_conversation(conn: &Connection, id: &str, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE conversations SET updated_at = MAX(updated_at, ?2) WHERE id = ?1",
        params![id, now],
    )?;
    Ok(())
}

fn i64_of(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

impl Store {
    /// Newest first.
    ///
    /// # Errors
    /// [`StoreError::ProjectNotFound`] or SQLite errors.
    pub fn list_conversations(&self, project_id: &str) -> Result<Vec<Conversation>> {
        self.get_project(project_id)?;
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {CONVERSATION_COLUMNS} FROM conversations WHERE project_id = ?1 \
                 ORDER BY updated_at DESC, id"
            ))?;
            let rows = stmt.query_map([project_id], conversation_from_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// # Errors
    /// [`StoreError::ProjectNotFound`], [`StoreError::BadRequest`] for an
    /// unsafe or taken id.
    pub fn create_conversation(
        &self,
        project_id: &str,
        req: CreateConversationRequest,
    ) -> Result<Conversation> {
        self.get_project(project_id)?;
        let id = match req.id {
            Some(id) if is_safe_id(&id) => id,
            Some(_) => return Err(StoreError::BadRequest("invalid conversation id".into())),
            None => uuid::Uuid::new_v4().to_string(),
        };
        let seed = match req.seed_from_conversation_id.as_deref() {
            Some(src) => Some(self.get_conversation(project_id, src)?.id),
            None => None,
        };
        let now = crate::now_ms();
        let mode = req.session_mode.unwrap_or_default();
        let title = req.title.filter(|t| !t.trim().is_empty());
        self.with_conn(|conn| {
            if get_conversation(conn, &id)?.is_some() {
                return Err(StoreError::BadRequest("conversation id already exists".into()));
            }
            let fork_upto = match (&seed, req.fork_after_message_id.as_deref()) {
                (Some(src), Some(mid)) => Some(fork_position(conn, src, mid)?),
                (Some(_), None) => Some(i64::MAX),
                (None, _) => None,
            };
            conn.execute(
                "INSERT INTO conversations (id, project_id, title, session_mode, created_at, updated_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![id, project_id, title, mode_str(mode), now],
            )?;
            if let (Some(src), Some(upto)) = (&seed, fork_upto) {
                copy_messages(conn, src, &id, upto)?;
            }
            Ok(Conversation {
                id: id.clone(),
                project_id: project_id.to_owned(),
                title,
                session_mode: mode,
                created_at: now,
                updated_at: now,
            })
        })
    }

    /// The conversation, checked to belong to `project_id`.
    ///
    /// # Errors
    /// [`StoreError::ConversationNotFound`] or SQLite errors.
    pub fn get_conversation(&self, project_id: &str, id: &str) -> Result<Conversation> {
        self.with_conn(|conn| get_conversation(conn, id))?
            .filter(|c| c.project_id == project_id)
            .ok_or(StoreError::ConversationNotFound)
    }

    /// Changes a conversation's session mode or title. The next run of
    /// the conversation uses the new mode.
    ///
    /// # Errors
    /// [`StoreError::ConversationNotFound`] or SQLite errors.
    pub fn update_conversation(
        &self,
        project_id: &str,
        id: &str,
        req: od_contracts::UpdateConversationRequest,
    ) -> Result<Conversation> {
        let mut conversation = self.get_conversation(project_id, id)?;
        if let Some(mode) = req.session_mode {
            conversation.session_mode = mode;
        }
        if let Some(title) = req.title {
            conversation.title = Some(title.trim().to_owned()).filter(|t| !t.is_empty());
        }
        conversation.updated_at = crate::now_ms().max(conversation.updated_at);
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE conversations SET session_mode = ?2, title = ?3, updated_at = ?4 WHERE id = ?1",
                params![
                    id,
                    mode_str(conversation.session_mode),
                    conversation.title,
                    conversation.updated_at
                ],
            )?;
            Ok(())
        })?;
        Ok(conversation)
    }

    /// Deletes a conversation; its messages, comments and agent sessions go
    /// with it (`ON DELETE CASCADE`). Returns the project's conversations
    /// left, newest first.
    ///
    /// # Errors
    /// [`StoreError::ConversationNotFound`] or SQLite errors.
    pub fn delete_conversation(&self, project_id: &str, id: &str) -> Result<Vec<Conversation>> {
        self.get_conversation(project_id, id)?;
        self.with_conn(|conn| {
            conn.execute("DELETE FROM conversations WHERE id = ?1", [id])?;
            Ok(())
        })?;
        self.list_conversations(project_id)
    }

    /// The project's most recently updated conversation, created when the
    /// project has none.
    ///
    /// # Errors
    /// Project or SQLite errors.
    pub fn latest_or_new_conversation(&self, project_id: &str) -> Result<Conversation> {
        match self.list_conversations(project_id)?.into_iter().next() {
            Some(c) => Ok(c),
            None => self.create_conversation(project_id, CreateConversationRequest::default()),
        }
    }

    /// In position order.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn list_messages(&self, conversation_id: &str) -> Result<Vec<Message>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {MESSAGE_COLUMNS} FROM messages WHERE conversation_id = ?1 ORDER BY position, created_at"
            ))?;
            let rows = stmt.query_map([conversation_id], message_from_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// # Errors
    /// SQLite errors.
    pub fn get_message(&self, id: &str) -> Result<Option<Message>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {MESSAGE_COLUMNS} FROM messages WHERE id = ?1"),
                    [id],
                    message_from_row,
                )
                .optional()?)
        })
    }

    /// Creates or replaces a client-authored message's content.
    ///
    /// # Errors
    /// [`StoreError::BadRequest`] for an unsafe id or a message of another
    /// conversation; SQLite errors.
    pub fn upsert_message(
        &self,
        conversation_id: &str,
        id: &str,
        role: MessageRole,
        content: &str,
        agent_id: Option<&str>,
    ) -> Result<Message> {
        if !is_safe_id(id) {
            return Err(StoreError::BadRequest("invalid message id".into()));
        }
        let now = crate::now_ms();
        self.with_conn(|conn| {
            let existing: Option<String> = conn
                .query_row(
                    "SELECT conversation_id FROM messages WHERE id = ?1",
                    [id],
                    |r| r.get(0),
                )
                .optional()?;
            match existing {
                Some(c) if c != conversation_id => {
                    return Err(StoreError::BadRequest(
                        "message belongs to another conversation".into(),
                    ));
                }
                Some(_) => {
                    conn.execute(
                        "UPDATE messages SET content = ?2, agent_id = COALESCE(?3, agent_id) WHERE id = ?1",
                        params![id, content, agent_id],
                    )?;
                }
                None => {
                    let position = next_position(conn, conversation_id)?;
                    let role = if role == MessageRole::Assistant {
                        "assistant"
                    } else {
                        "user"
                    };
                    conn.execute(
                        "INSERT INTO messages (id, conversation_id, role, content, agent_id, position, created_at) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                        params![id, conversation_id, role, content, agent_id, position, now],
                    )?;
                }
            }
            touch_conversation(conn, conversation_id, now)?;
            Ok(())
        })?;
        self.get_message(id)?
            .ok_or_else(|| StoreError::BadRequest("message vanished".into()))
    }

    /// Inserts the user message and the running assistant placeholder in
    /// one transaction.
    ///
    /// # Errors
    /// [`StoreError::BadRequest`] for unsafe or existing ids.
    pub fn begin_turn(&self, turn: &NewTurn<'_>) -> Result<()> {
        if !is_safe_id(turn.user_message_id) || !is_safe_id(turn.assistant_message_id) {
            return Err(StoreError::BadRequest("invalid message id".into()));
        }
        let now = crate::now_ms();
        let attachments = serde_json::to_string(turn.attachments)?;
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                for id in [turn.user_message_id, turn.assistant_message_id] {
                    let taken: Option<i64> = conn
                        .query_row("SELECT 1 FROM messages WHERE id = ?1", [id], |r| r.get(0))
                        .optional()?;
                    if taken.is_some() {
                        return Err(StoreError::BadRequest(format!("message {id} already exists")));
                    }
                }
                let position = next_position(conn, turn.conversation_id)?;
                conn.execute(
                    "INSERT INTO messages (id, conversation_id, role, content, agent_id, position, \
                     attachments_json, created_at) VALUES (?1, ?2, 'user', ?3, ?4, ?5, ?6, ?7)",
                    params![
                        turn.user_message_id,
                        turn.conversation_id,
                        turn.content,
                        turn.agent_id,
                        position,
                        attachments,
                        now
                    ],
                )?;
                conn.execute(
                    "INSERT INTO messages (id, conversation_id, role, content, agent_id, run_id, run_status, \
                     position, started_at, created_at) VALUES (?1, ?2, 'assistant', '', ?3, ?4, 'running', ?5, ?6, ?6)",
                    params![
                        turn.assistant_message_id,
                        turn.conversation_id,
                        turn.agent_id,
                        turn.run_id,
                        position + 1,
                        now
                    ],
                )?;
                touch_conversation(conn, turn.conversation_id, now)
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(err)
                }
            }
        })
    }

    /// Appends one batch of `agent` payloads written while the run streams.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn append_event_batch(
        &self,
        message_id: &str,
        run_id: &str,
        seq: i64,
        events: &[Value],
        last_event_id: u64,
    ) -> Result<()> {
        let json = serde_json::to_string(events)?;
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO message_event_batches \
                 (message_id, run_id, seq, events_json, last_event_id, created_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    message_id,
                    run_id,
                    seq,
                    json,
                    i64_of(last_event_id),
                    crate::now_ms()
                ],
            )?;
            conn.execute(
                "UPDATE messages SET last_run_event_id = ?2 WHERE id = ?1",
                params![message_id, i64_of(last_event_id)],
            )?;
            Ok(())
        })
    }

    /// Ends the assistant message: folds its batches into `events_json`,
    /// deletes them, stores the final text and status.
    ///
    /// # Errors
    /// SQLite or JSON errors.
    pub fn finish_assistant_message(
        &self,
        message_id: &str,
        content: &str,
        status: RunStatus,
        last_event_id: u64,
    ) -> Result<()> {
        let now = crate::now_ms();
        self.with_conn(|conn| {
            conn.execute_batch("BEGIN IMMEDIATE")?;
            let result = (|| {
                let mut stmt = conn.prepare(
                    "SELECT events_json FROM message_event_batches WHERE message_id = ?1 ORDER BY seq",
                )?;
                let batches = stmt
                    .query_map([message_id], |r| r.get::<_, String>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                drop(stmt);
                let mut events: Vec<Value> = Vec::new();
                for batch in batches {
                    events.extend(serde_json::from_str::<Vec<Value>>(&batch)?);
                }
                conn.execute(
                    "UPDATE messages SET content = ?2, run_status = ?3, last_run_event_id = ?4, \
                     events_json = ?5, ended_at = ?6 WHERE id = ?1",
                    params![
                        message_id,
                        content,
                        status.as_str(),
                        i64_of(last_event_id),
                        serde_json::to_string(&events)?,
                        now
                    ],
                )?;
                conn.execute(
                    "DELETE FROM message_event_batches WHERE message_id = ?1",
                    [message_id],
                )?;
                Ok(())
            })();
            match result {
                Ok(()) => {
                    conn.execute_batch("COMMIT")?;
                    Ok(())
                }
                Err(err) => {
                    let _ = conn.execute_batch("ROLLBACK");
                    Err(err)
                }
            }
        })
    }

    /// At startup: messages still marked running belong to runs that died
    /// with the daemon. Their batches are folded in, closed by an `error`
    /// event coded [`od_contracts::DAEMON_RESTART_CODE`] (so the chat can
    /// say Contour closed, without the run's log), and they become failed.
    ///
    /// # Errors
    /// SQLite or JSON errors.
    pub fn fail_interrupted_messages(&self) -> Result<usize> {
        type Row = (String, String, Option<i64>, Option<String>);
        let ids: Vec<Row> = self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, content, last_run_event_id, run_id FROM messages \
                 WHERE run_status = 'running'",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })?;
        let cause = serde_json::to_value(od_contracts::AgentEvent::Error {
            message: "the daemon restarted while this run was in progress".to_owned(),
            code: Some(od_contracts::DAEMON_RESTART_CODE.to_owned()),
            terminal: true,
        })?;
        for (id, content, last, run_id) in &ids {
            let last = last.and_then(|v| u64::try_from(v).ok()).unwrap_or(0);
            self.append_event_batch(
                id,
                run_id.as_deref().unwrap_or_default(),
                i64::MAX,
                std::slice::from_ref(&cause),
                last,
            )?;
            self.finish_assistant_message(id, content, RunStatus::Failed, last)?;
        }
        // Comments a dead run was applying go back to the next turn.
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE preview_comments SET status = 'attached', updated_at = ?1 \
                 WHERE status = 'applying'",
                [crate::now_ms()],
            )?;
            Ok(())
        })?;
        Ok(ids.len())
    }

    /// # Errors
    /// SQLite errors.
    pub fn get_agent_session(
        &self,
        conversation_id: &str,
        agent_id: &str,
    ) -> Result<Option<AgentSession>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT session_id, stable_prompt_hash, model, cwd, last_message_id \
                     FROM agent_sessions WHERE conversation_id = ?1 AND agent_id = ?2",
                    params![conversation_id, agent_id],
                    |r| {
                        Ok(AgentSession {
                            conversation_id: conversation_id.to_owned(),
                            agent_id: agent_id.to_owned(),
                            session_id: r.get(0)?,
                            stable_prompt_hash: r.get(1)?,
                            model: r.get(2)?,
                            cwd: r.get(3)?,
                            last_message_id: r.get(4)?,
                        })
                    },
                )
                .optional()?)
        })
    }

    /// # Errors
    /// SQLite errors.
    pub fn save_agent_session(&self, s: &AgentSession) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO agent_sessions (conversation_id, agent_id, session_id, stable_prompt_hash, \
                 model, cwd, last_message_id, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
                 ON CONFLICT(conversation_id, agent_id) DO UPDATE SET session_id = excluded.session_id, \
                 stable_prompt_hash = excluded.stable_prompt_hash, model = excluded.model, \
                 cwd = excluded.cwd, last_message_id = excluded.last_message_id, \
                 updated_at = excluded.updated_at",
                params![
                    s.conversation_id,
                    s.agent_id,
                    s.session_id,
                    s.stable_prompt_hash,
                    s.model,
                    s.cwd,
                    s.last_message_id,
                    crate::now_ms()
                ],
            )?;
            Ok(())
        })
    }

    /// # Errors
    /// SQLite errors.
    pub fn delete_agent_session(&self, conversation_id: &str, agent_id: &str) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM agent_sessions WHERE conversation_id = ?1 AND agent_id = ?2",
                params![conversation_id, agent_id],
            )?;
            Ok(())
        })
    }

    /// The newest assistant message that finished successfully. The
    /// current run's placeholder is `running`, so it never qualifies.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn latest_completed_assistant_id(&self, conversation_id: &str) -> Result<Option<String>> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT id FROM messages WHERE conversation_id = ?1 AND role = 'assistant' \
                     AND (run_status IS NULL OR run_status = 'succeeded') \
                     ORDER BY position DESC LIMIT 1",
                    params![conversation_id],
                    |r| r.get(0),
                )
                .optional()?)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataDir;
    use od_contracts::CreateProjectRequest;
    use serde_json::json;

    #[test]
    fn fork_copies_messages_up_to_the_fork_point() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let (project, conv) = store
            .create_project(CreateProjectRequest {
                name: "p".into(),
                ..CreateProjectRequest::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        for (u, a, run) in [("u1", "a1", "r1"), ("u2", "a2", "r2")] {
            store
                .begin_turn(&NewTurn {
                    conversation_id: &conv,
                    user_message_id: u,
                    assistant_message_id: a,
                    content: u,
                    attachments: &[],
                    agent_id: "claude",
                    run_id: run,
                })
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let fork = |after: Option<&str>| {
            store.create_conversation(
                &project.id,
                CreateConversationRequest {
                    seed_from_conversation_id: Some(conv.clone()),
                    fork_after_message_id: after.map(str::to_owned),
                    ..CreateConversationRequest::default()
                },
            )
        };
        let first = fork(Some("a1")).unwrap_or_else(|e| panic!("{e}"));
        let copied = store
            .list_messages(&first.id)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            copied
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>(),
            ["u1", ""]
        );
        assert!(
            copied.iter().all(|m| m.id != "u1" && m.id != "a1"),
            "fresh ids"
        );
        assert_eq!(
            copied[1].run_status,
            Some(RunStatus::Canceled),
            "a running turn is not running in the copy"
        );
        let all = fork(None).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(store.list_messages(&all.id).map(|m| m.len()).ok(), Some(4));
        let before = store.list_conversations(&project.id).map(|c| c.len()).ok();
        assert!(matches!(fork(Some("nope")), Err(StoreError::BadRequest(_))));
        assert_eq!(
            store.list_conversations(&project.id).map(|c| c.len()).ok(),
            before,
            "a failed fork leaves no conversation behind"
        );
        assert_eq!(
            store.list_messages(&conv).map(|m| m.len()).ok(),
            Some(4),
            "the source is untouched"
        );
    }

    #[test]
    fn delete_conversation_takes_its_messages_and_stays_in_its_project() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let new_project = || {
            store
                .create_project(CreateProjectRequest {
                    name: "p".into(),
                    ..CreateProjectRequest::default()
                })
                .unwrap_or_else(|e| panic!("{e}"))
        };
        let (project, conv) = new_project();
        let (other, _) = new_project();
        store
            .begin_turn(&NewTurn {
                conversation_id: &conv,
                user_message_id: "u1",
                assistant_message_id: "a1",
                content: "hi",
                attachments: &[],
                agent_id: "claude",
                run_id: "r1",
            })
            .unwrap_or_else(|e| panic!("{e}"));
        let second = store
            .create_conversation(&project.id, CreateConversationRequest::default())
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(
            store.delete_conversation(&other.id, &conv),
            Err(StoreError::ConversationNotFound)
        ));
        let left = store
            .delete_conversation(&project.id, &conv)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            left.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(),
            [second.id.as_str()]
        );
        assert_eq!(store.list_messages(&conv).map(|m| m.len()).ok(), Some(0));
        assert!(matches!(
            store.delete_conversation(&project.id, &conv),
            Err(StoreError::ConversationNotFound)
        ));
    }

    #[test]
    fn turn_batches_fold_into_events_json() {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let (project, conv) = store
            .create_project(CreateProjectRequest {
                name: "p".into(),
                ..CreateProjectRequest::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .begin_turn(&NewTurn {
                conversation_id: &conv,
                user_message_id: "u1",
                assistant_message_id: "a1",
                content: "hi",
                attachments: &[],
                agent_id: "claude",
                run_id: "r1",
            })
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .append_event_batch(
                "a1",
                "r1",
                0,
                &[json!({"type":"text_delta","delta":"he"})],
                3,
            )
            .unwrap_or_else(|e| panic!("{e}"));
        store
            .append_event_batch(
                "a1",
                "r1",
                1,
                &[json!({"type":"text_delta","delta":"llo"})],
                5,
            )
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(store.fail_interrupted_messages().unwrap_or(0), 1);
        let messages = store.list_messages(&conv).unwrap_or_default();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].run_status, Some(RunStatus::Failed));
        // The cause goes with the turn (QA-030): the chat tells "Contour
        // closed" apart from an agent error without the run's log.
        assert_eq!(messages[1].events.len(), 3);
        assert_eq!(messages[1].events[2]["type"], "error");
        assert_eq!(
            messages[1].events[2]["code"],
            od_contracts::DAEMON_RESTART_CODE
        );
        assert_eq!(messages[1].last_run_event_id, Some(5));
        assert_eq!(
            store
                .latest_completed_assistant_id(&conv)
                .unwrap_or_default(),
            None
        );
        assert_eq!(
            store
                .list_conversations(&project.id)
                .unwrap_or_default()
                .len(),
            1
        );

        // Ids are never reused, and a message stays in its conversation.
        let again = store.begin_turn(&NewTurn {
            conversation_id: &conv,
            user_message_id: "u2",
            assistant_message_id: "a1",
            content: "x",
            attachments: &[],
            agent_id: "claude",
            run_id: "r2",
        });
        assert!(matches!(again, Err(StoreError::BadRequest(_))));
        assert_eq!(store.list_messages(&conv).unwrap_or_default().len(), 2);
        let other = store
            .create_conversation(&project.id, CreateConversationRequest::default())
            .unwrap_or_else(|e| panic!("{e}"));
        let moved = store.upsert_message(&other.id, "u1", MessageRole::User, "hijack", None);
        assert!(matches!(moved, Err(StoreError::BadRequest(_))));
    }

    #[test]
    fn a_dead_run_hands_its_comments_back_and_mode_changes_stick() {
        use od_contracts::{CommentStatus, CreateCommentRequest, UpdateConversationRequest};
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let (project, conv) = store
            .create_project(CreateProjectRequest {
                name: "p".into(),
                ..CreateProjectRequest::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        let comment = |status| {
            store
                .create_comment(
                    &project.id,
                    &conv,
                    CreateCommentRequest {
                        file_path: "index.html".into(),
                        element_id: String::new(),
                        selector: "h1".into(),
                        label: String::new(),
                        text: String::new(),
                        position: od_contracts::CommentRect::default(),
                        html_hint: String::new(),
                        slide_index: None,
                        note: "bigger".into(),
                        status: Some(status),
                        attachments: Vec::new(),
                    },
                )
                .unwrap_or_else(|e| panic!("{e}"))
        };
        let applying = comment(CommentStatus::Applying);
        let resolved = comment(CommentStatus::Resolved);
        store
            .begin_turn(&NewTurn {
                conversation_id: &conv,
                user_message_id: "u1",
                assistant_message_id: "a1",
                content: "fix it",
                attachments: &[],
                agent_id: "claude",
                run_id: "r1",
            })
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(store.fail_interrupted_messages().ok(), Some(1));
        let status = |id: &str| {
            store
                .list_comments(&project.id, &conv)
                .unwrap_or_default()
                .into_iter()
                .find(|c| c.id == id)
                .map(|c| c.status)
        };
        assert_eq!(status(&applying.id), Some(CommentStatus::Attached));
        assert_eq!(status(&resolved.id), Some(CommentStatus::Resolved));

        let changed = store
            .update_conversation(
                &project.id,
                &conv,
                UpdateConversationRequest {
                    session_mode: Some(SessionMode::Plan),
                    title: Some("  Plan  ".into()),
                },
            )
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(changed.title.as_deref(), Some("Plan"));
        let read = store
            .get_conversation(&project.id, &conv)
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read.session_mode, SessionMode::Plan);
        assert!(matches!(
            store.update_conversation(&project.id, "nope", UpdateConversationRequest::default()),
            Err(StoreError::ConversationNotFound)
        ));
    }
}
