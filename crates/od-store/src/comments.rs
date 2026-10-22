//! Preview comments (`preview_comments`, spec 3.3), scoped to a conversation.

use crate::error::{Result, StoreError};
use crate::paths::validate_project_path;
use crate::store::Store;
use od_contracts::{
    CommentRect, CommentStatus, CreateCommentRequest, PreviewComment, UpdateCommentRequest,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

/// Longest note, element text and HTML hint kept (characters).
const MAX_NOTE: usize = 4000;
const MAX_TEXT: usize = 400;
const MAX_HINT: usize = 400;

const COLUMNS: &str = "id, project_id, conversation_id, file_path, element_id, selector, label, \
     text, position_json, html_hint, slide_index, note, status, created_at, updated_at";

fn clip(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<PreviewComment> {
    let position: String = row.get(8)?;
    let slide: Option<i64> = row.get(10)?;
    let status: String = row.get(12)?;
    Ok(PreviewComment {
        id: row.get(0)?,
        project_id: row.get(1)?,
        conversation_id: row.get(2)?,
        file_path: row.get(3)?,
        element_id: row.get(4)?,
        selector: row.get(5)?,
        label: row.get(6)?,
        text: row.get(7)?,
        position: serde_json::from_str::<CommentRect>(&position).unwrap_or_default(),
        html_hint: row.get(9)?,
        slide_index: slide.and_then(|s| u32::try_from(s).ok()),
        note: row.get(11)?,
        status: CommentStatus::parse(&status),
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
    })
}

fn get(conn: &Connection, conversation_id: &str, id: &str) -> Result<PreviewComment> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM preview_comments WHERE id = ?1 AND conversation_id = ?2"),
        params![id, conversation_id],
        from_row,
    )
    .optional()?
    .ok_or(StoreError::CommentNotFound)
}

impl Store {
    /// Comments of one conversation, oldest first.
    ///
    /// # Errors
    /// [`StoreError::ConversationNotFound`] or SQLite errors.
    pub fn list_comments(
        &self,
        project_id: &str,
        conversation_id: &str,
    ) -> Result<Vec<PreviewComment>> {
        self.get_conversation(project_id, conversation_id)?;
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLUMNS} FROM preview_comments WHERE conversation_id = ?1 \
                 ORDER BY created_at, rowid"
            ))?;
            let rows = stmt.query_map([conversation_id], from_row)?;
            Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
        })
    }

    /// # Errors
    /// [`StoreError::BadRequest`] for an empty note or selector, path errors,
    /// [`StoreError::ConversationNotFound`], SQLite errors.
    pub fn create_comment(
        &self,
        project_id: &str,
        conversation_id: &str,
        req: CreateCommentRequest,
    ) -> Result<PreviewComment> {
        self.get_conversation(project_id, conversation_id)?;
        let file_path = validate_project_path(&req.file_path)?;
        let note = clip(&req.note, MAX_NOTE);
        if note.is_empty() {
            return Err(StoreError::BadRequest("note required".into()));
        }
        if req.selector.trim().is_empty() {
            return Err(StoreError::BadRequest("selector required".into()));
        }
        let now = crate::now_ms();
        let comment = PreviewComment {
            id: uuid::Uuid::new_v4().to_string(),
            project_id: project_id.to_owned(),
            conversation_id: conversation_id.to_owned(),
            file_path,
            element_id: req.element_id,
            selector: req.selector,
            label: clip(&req.label, 64),
            text: clip(&req.text, MAX_TEXT),
            position: req.position,
            html_hint: clip(&req.html_hint, MAX_HINT),
            slide_index: req.slide_index,
            note,
            status: req.status.unwrap_or_default(),
            created_at: now,
            updated_at: now,
        };
        let position = serde_json::to_string(&comment.position)?;
        self.with_conn(|conn| {
            conn.execute(
                &format!(
                    "INSERT INTO preview_comments ({COLUMNS}) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)"
                ),
                params![
                    comment.id,
                    comment.project_id,
                    comment.conversation_id,
                    comment.file_path,
                    comment.element_id,
                    comment.selector,
                    comment.label,
                    comment.text,
                    position,
                    comment.html_hint,
                    comment.slide_index,
                    comment.note,
                    comment.status.as_str(),
                    now,
                ],
            )?;
            Ok(())
        })?;
        Ok(comment)
    }

    /// # Errors
    /// [`StoreError::CommentNotFound`], [`StoreError::BadRequest`] for an
    /// empty note, SQLite errors.
    pub fn update_comment(
        &self,
        project_id: &str,
        conversation_id: &str,
        id: &str,
        req: UpdateCommentRequest,
    ) -> Result<PreviewComment> {
        self.get_conversation(project_id, conversation_id)?;
        let note = req.note.map(|n| clip(&n, MAX_NOTE));
        if note.as_deref().is_some_and(str::is_empty) {
            return Err(StoreError::BadRequest("note required".into()));
        }
        self.with_conn(|conn| {
            let mut comment = get(conn, conversation_id, id)?;
            if let Some(note) = note {
                comment.note = note;
            }
            if let Some(status) = req.status {
                comment.status = status;
            }
            comment.updated_at = crate::now_ms();
            conn.execute(
                "UPDATE preview_comments SET note = ?2, status = ?3, updated_at = ?4 WHERE id = ?1",
                params![
                    comment.id,
                    comment.note,
                    comment.status.as_str(),
                    comment.updated_at
                ],
            )?;
            Ok(comment)
        })
    }

    /// # Errors
    /// [`StoreError::CommentNotFound`] or SQLite errors.
    pub fn delete_comment(&self, project_id: &str, conversation_id: &str, id: &str) -> Result<()> {
        self.get_conversation(project_id, conversation_id)?;
        self.with_conn(|conn| {
            let n = conn.execute(
                "DELETE FROM preview_comments WHERE id = ?1 AND conversation_id = ?2",
                params![id, conversation_id],
            )?;
            if n == 0 {
                return Err(StoreError::CommentNotFound);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DataDir;
    use od_contracts::CreateProjectRequest;

    fn store() -> (tempfile::TempDir, Store, String, String) {
        let tmp = tempfile::tempdir().unwrap_or_else(|e| panic!("{e}"));
        let store = Store::open(DataDir::new(tmp.path())).unwrap_or_else(|e| panic!("{e}"));
        let (project, conv) = store
            .create_project(CreateProjectRequest {
                name: "p".into(),
                ..CreateProjectRequest::default()
            })
            .unwrap_or_else(|e| panic!("{e}"));
        (tmp, store, project.id, conv)
    }

    fn req(note: &str) -> CreateCommentRequest {
        CreateCommentRequest {
            file_path: "index.html".into(),
            selector: "html > body:nth-of-type(1) > h1:nth-of-type(1)".into(),
            label: "h1".into(),
            text: "  Title  ".into(),
            position: CommentRect {
                x: 1.0,
                y: 2.0,
                width: 30.0,
                height: 10.0,
            },
            note: note.into(),
            ..CreateCommentRequest::default()
        }
    }

    #[test]
    fn create_list_update_delete() {
        let (_tmp, s, p, c) = store();
        let made = s
            .create_comment(&p, &c, req("Make it red"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            made.status,
            CommentStatus::Attached,
            "new comments go with the next turn"
        );
        assert_eq!(made.text, "Title");
        let listed = s.list_comments(&p, &c).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(listed, vec![made.clone()]);

        let updated = s
            .update_comment(
                &p,
                &c,
                &made.id,
                UpdateCommentRequest {
                    status: Some(CommentStatus::Applying),
                    note: None,
                },
            )
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(updated.status, CommentStatus::Applying);
        assert_eq!(updated.note, "Make it red");
        let reread = s.list_comments(&p, &c).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(reread[0].status, CommentStatus::Applying);
        assert_eq!(reread[0].position.width, 30.0);

        s.delete_comment(&p, &c, &made.id)
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(
            s.delete_comment(&p, &c, &made.id),
            Err(StoreError::CommentNotFound)
        ));
    }

    #[test]
    fn rejects_bad_input_and_foreign_conversations() {
        let (_tmp, s, p, c) = store();
        assert!(matches!(
            s.create_comment(&p, &c, req("   ")),
            Err(StoreError::BadRequest(_))
        ));
        let mut escape = req("x");
        escape.file_path = "../x.html".into();
        assert!(s.create_comment(&p, &c, escape).is_err());
        assert!(matches!(
            s.create_comment(&p, "nope", req("x")),
            Err(StoreError::ConversationNotFound)
        ));
        let made = s
            .create_comment(&p, &c, req("x"))
            .unwrap_or_else(|e| panic!("{e}"));
        let other = s
            .create_conversation(&p, od_contracts::CreateConversationRequest::default())
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(
            s.update_comment(&p, &other.id, &made.id, UpdateCommentRequest::default()),
            Err(StoreError::CommentNotFound)
        ));
    }
}
