//! Crystallize (spec 1.5 "Automations"): a succeeded routine run becomes
//! reviewable evolution proposals (a user skill and a memory entry). Port
//! of the original `automation-ingestions.ts` (crystallize template) and
//! `automation-proposals.ts`.
//!
//! - [`crystallize`] turns a run into a source packet and its proposals,
//!   without side effects.
//! - [`EvolutionStore`] keeps packets and proposals as JSON files under
//!   `<data>/automation-source-packets/` and `<data>/automation-proposals/`
//!   (the original's layout) and gates review: only a `pending-review`
//!   proposal can be applied or rejected, once.
//!
//! Applying (writing the skill or memory) belongs to the daemon, which
//! owns those stores; it runs inside [`EvolutionStore::review`].

use crate::store::{Routine, RoutineRun};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

const PACKETS: &str = "automation-source-packets/packets.json";
const PROPOSALS: &str = "automation-proposals/proposals.json";
/// The `balanced` compression budget of the original (characters).
const BALANCED_MAX_CHARS: usize = 3_200;
/// What a memory proposal keeps of the run (global memory caps at 1000).
const MEMORY_MAX_CHARS: usize = 900;
/// Id of the original's built-in template this flow follows.
pub const CRYSTALLIZE_TEMPLATE: &str = "crystallize-run-into-skill";

#[derive(Debug, thiserror::Error)]
pub enum EvolutionError {
    #[error("{0}")]
    Invalid(String),
    #[error("automation proposal not found")]
    NotFound,
    #[error("proposal {id} is {status}, not reviewable")]
    NotReviewable { id: String, status: &'static str },
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T, E = EvolutionError> = std::result::Result<T, E>;

/// What a proposal changes once applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetKind {
    MemoryNode,
    Skill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProposalStatus {
    PendingReview,
    Applied,
    Rejected,
}

impl ProposalStatus {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PendingReview => "pending-review",
            Self::Applied => "applied",
            Self::Rejected => "rejected",
        }
    }
}

/// The change, as the original's `patch`: `after` is the full new text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Patch {
    pub format: String,
    pub after: String,
    pub diff_summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CompressionReport {
    pub mode: String,
    pub status: String,
    pub before_tokens: usize,
    pub after_tokens: usize,
    pub summary: String,
    pub preserved_source_packet_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub id: String,
    pub title: String,
    pub summary: String,
    pub target_kind: TargetKind,
    pub action: String,
    pub status: ProposalStatus,
    pub review_policy: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub source_packet_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<String>,
    pub patch: Patch,
    pub compression_report: CompressionReport,
    /// Routine, run and what the applier needs (`slug`, `name`,
    /// `description`, `memoryType`); after review also `appliedResult` or
    /// `rejectedReason`.
    pub metadata: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Packet {
    pub id: String,
    pub source_kind: String,
    pub source_ref: String,
    pub title: String,
    pub captured_at: i64,
    pub body_markdown: String,
    pub token_stats: Value,
    pub candidate_sinks: Vec<String>,
    pub metadata: Value,
}

/// What [`crystallize`] produced.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Crystallized {
    pub packet: Packet,
    pub compression_report: CompressionReport,
    pub proposals: Vec<Proposal>,
}

/// The original's estimate: four characters a token.
fn estimate_tokens(text: &str) -> usize {
    if text.trim().is_empty() {
        0
    } else {
        text.chars().count().div_ceil(4).max(1)
    }
}

fn take_chars(text: &str, n: usize) -> &str {
    text.char_indices().nth(n).map_or(text, |(i, _)| &text[..i])
}

/// `balanced` compression: keep the head, note what was cut.
fn compact(body: &str, packet_id: &str) -> (String, CompressionReport) {
    let before = estimate_tokens(body);
    let report = |status: &str, after: usize, summary: &str| CompressionReport {
        mode: "balanced".to_owned(),
        status: status.to_owned(),
        before_tokens: before,
        after_tokens: after,
        summary: summary.to_owned(),
        preserved_source_packet_id: packet_id.to_owned(),
    };
    if body.chars().count() <= BALANCED_MAX_CHARS {
        let r = report(
            "skipped",
            before,
            "Source packet was already below the compression threshold.",
        );
        return (body.to_owned(), r);
    }
    let head = take_chars(body, BALANCED_MAX_CHARS).trim_end();
    let omitted = estimate_tokens(&body[head.len()..]);
    let out = format!(
        "{head}\n\n> Automation compression preserved the original packet ({packet_id}) and omitted roughly {omitted} tokens from this proposal preview."
    );
    let r = report(
        "applied",
        estimate_tokens(&out),
        "Trimmed oversized source context while preserving provenance to the full packet.",
    );
    (out, r)
}

/// Lowercase ASCII words joined by `-`, at most 60 characters.
fn slugify(text: &str) -> String {
    let mut out = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let out: String = out.trim_end_matches('-').chars().take(60).collect();
    let out = out.trim_end_matches('-').to_owned();
    if out.is_empty() {
        "crystallized-run".to_owned()
    } else {
        out
    }
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4())
}

/// Turns a succeeded routine run into a packet plus a skill proposal and a
/// memory proposal. `summary` is the run's final assistant text (empty
/// when there was none).
///
/// # Errors
/// [`EvolutionError::Invalid`] when the run did not succeed or belongs to
/// another routine.
pub fn crystallize(
    routine: &Routine,
    run: &RoutineRun,
    summary: &str,
    now: i64,
) -> Result<Crystallized> {
    if run.routine_id != routine.id {
        return Err(EvolutionError::Invalid("routine run not found".to_owned()));
    }
    if run.status != crate::RunState::Succeeded {
        return Err(EvolutionError::Invalid(
            "only succeeded routine runs can be crystallized".to_owned(),
        ));
    }
    let summary = summary.trim();
    let summary = if summary.is_empty() {
        "No run summary was recorded; crystallize from the automation prompt and run metadata."
    } else {
        summary
    };
    let opt = |v: &Option<String>| v.clone().unwrap_or_default();
    let body = [
        format!("# {} reusable workflow", routine.name),
        String::new(),
        format!("Routine id: {}", routine.id),
        format!("Routine run: {}", run.id),
        format!("Project id: {}", opt(&run.project_id)),
        format!("Conversation id: {}", opt(&run.conversation_id)),
        format!("Agent run id: {}", opt(&run.run_id)),
        String::new(),
        "## Original Automation Prompt".to_owned(),
        String::new(),
        routine.prompt.clone(),
        String::new(),
        "## Run Summary".to_owned(),
        String::new(),
        summary.to_owned(),
    ]
    .join("\n");
    let packet_id = new_id("packet");
    let title = format!("{} run", routine.name);
    let source_ref = format!("routine-run:{}", run.id);
    let (proposal_body, report) = compact(&body, &packet_id);
    let tokens = estimate_tokens(&body);
    let base_meta = json!({
        "templateId": CRYSTALLIZE_TEMPLATE,
        "routineId": routine.id,
        "routineRunId": run.id,
        "agentRunId": run.run_id,
        "projectId": run.project_id,
        "conversationId": run.conversation_id,
    });
    let packet = Packet {
        id: packet_id.clone(),
        source_kind: "chat".to_owned(),
        source_ref: source_ref.clone(),
        title: title.clone(),
        captured_at: now,
        body_markdown: body,
        token_stats: json!({
            "originalTokens": tokens,
            "canonicalTokens": tokens,
            "compressedTokens": report.after_tokens,
        }),
        candidate_sinks: vec!["skill".to_owned(), "memory".to_owned()],
        metadata: base_meta.clone(),
    };

    let slug = slugify(&title);
    let skill_name = format!("{title} skill");
    let skill_description = format!("Self-evolved from chat {source_ref}");
    let skill_body = format!(
        "# {skill_name}\n\nSource packet: {packet_id}\n\n## When To Use\n\nUse this skill when a future request matches the workflow or reusable design guidance below.\n\n## Workflow\n\n{proposal_body}"
    );
    let skill_md = format!(
        "---\nname: {}\ndescription: {}\ntriggers:\n  - {}\n---\n\n{skill_body}\n",
        yaml_quoted(&skill_name),
        yaml_quoted(&skill_description),
        yaml_quoted(&title),
    );
    let mut skill_meta = base_meta.clone();
    merge(
        &mut skill_meta,
        json!({
            "sourceKind": "chat",
            "sourceRef": source_ref,
            "slug": slug,
            "name": skill_name,
            "description": skill_description,
            "triggers": [title],
            "body": skill_body,
        }),
    );

    let memory_text = format!(
        "{title}: {}",
        take_chars(
            summary
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .as_str(),
            MEMORY_MAX_CHARS
        )
    );
    let mut memory_meta = base_meta;
    merge(
        &mut memory_meta,
        json!({ "sourceKind": "chat", "sourceRef": source_ref, "memoryType": "project" }),
    );
    let proposal = |kind: TargetKind, title: String, summary: String, target_ref, patch| Proposal {
        id: new_id("proposal"),
        title,
        summary,
        target_kind: kind,
        action: "create".to_owned(),
        status: ProposalStatus::PendingReview,
        review_policy: "always".to_owned(),
        created_at: now,
        updated_at: now,
        source_packet_ids: vec![packet_id.clone()],
        target_ref,
        patch,
        compression_report: report.clone(),
        metadata: Value::Null,
    };
    let mut skill = proposal(
        TargetKind::Skill,
        format!("Skill: {title}"),
        format!("Draft a reusable SKILL.md proposal from chat source {source_ref}."),
        Some(format!("skills/{slug}/SKILL.md")),
        Patch {
            format: "markdown".to_owned(),
            after: skill_md,
            diff_summary: "Creates a reviewable SKILL.md draft from the source packet.".to_owned(),
        },
    );
    skill.metadata = skill_meta;
    let mut memory = proposal(
        TargetKind::MemoryNode,
        format!("Memory: {title}"),
        format!("Create a memory entry from chat source {source_ref}."),
        None,
        Patch {
            format: "text".to_owned(),
            after: memory_text,
            diff_summary: "Creates one editable memory entry from the source packet.".to_owned(),
        },
    );
    memory.metadata = memory_meta;
    Ok(Crystallized {
        packet,
        compression_report: report,
        proposals: vec![skill, memory],
    })
}

fn yaml_quoted(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

fn merge(into: &mut Value, more: Value) {
    if let (Some(a), Value::Object(b)) = (into.as_object_mut(), more) {
        a.extend(b);
    }
}

/// Packets and proposals on disk. One lock serialises every
/// read-modify-write, including a whole review.
#[derive(Debug)]
pub struct EvolutionStore {
    root: PathBuf,
    lock: Mutex<()>,
}

#[derive(Serialize, Deserialize, Default)]
struct ProposalFile {
    proposals: Vec<Proposal>,
}

#[derive(Serialize, Deserialize, Default)]
struct PacketFile {
    packets: Vec<Packet>,
}

fn read_json<T: serde::de::DeserializeOwned + Default>(path: &Path) -> Result<T> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(e.into()),
    }
}

/// Writes through a temporary file, so a crash leaves the old file whole.
fn write_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// The review decision [`EvolutionStore::review`] records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Review {
    /// Apply: the applier's result is kept as `appliedResult`.
    Apply,
    Reject {
        reason: Option<String>,
    },
}

impl EvolutionStore {
    /// A store over the data folder `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            lock: Mutex::new(()),
        }
    }

    fn guard(&self) -> std::sync::MutexGuard<'_, ()> {
        self.lock.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Saves what [`crystallize`] produced.
    ///
    /// # Errors
    /// I/O and JSON errors.
    pub fn save(&self, c: &Crystallized) -> Result<()> {
        let _g = self.guard();
        let path = self.root.join(PACKETS);
        let mut packets: PacketFile = read_json(&path)?;
        packets.packets.push(c.packet.clone());
        write_json(&path, &packets)?;
        let path = self.root.join(PROPOSALS);
        let mut file: ProposalFile = read_json(&path)?;
        file.proposals.extend(c.proposals.iter().cloned());
        write_json(&path, &file)
    }

    /// Proposals with `status` (all when `None`), most recently updated
    /// first.
    ///
    /// # Errors
    /// I/O and JSON errors.
    pub fn proposals(&self, status: Option<ProposalStatus>) -> Result<Vec<Proposal>> {
        let _g = self.guard();
        let file: ProposalFile = read_json(&self.root.join(PROPOSALS))?;
        let mut out: Vec<Proposal> = file
            .proposals
            .into_iter()
            .filter(|p| status.is_none_or(|s| p.status == s))
            .collect();
        out.sort_by_key(|a| std::cmp::Reverse(a.updated_at));
        Ok(out)
    }

    /// # Errors
    /// [`EvolutionError::NotFound`], I/O and JSON errors.
    pub fn proposal(&self, id: &str) -> Result<Proposal> {
        self.proposals(None)?
            .into_iter()
            .find(|p| p.id == id)
            .ok_or(EvolutionError::NotFound)
    }

    /// Packets, newest first.
    ///
    /// # Errors
    /// I/O and JSON errors.
    pub fn packets(&self) -> Result<Vec<Packet>> {
        let _g = self.guard();
        let file: PacketFile = read_json(&self.root.join(PACKETS))?;
        let mut out = file.packets;
        out.sort_by_key(|a| std::cmp::Reverse(a.captured_at));
        Ok(out)
    }

    /// Reviews proposal `id` once. For [`Review::Apply`], `apply` writes
    /// the change and returns its result; it runs under the store's lock,
    /// so two reviews of one proposal never both apply. A failed `apply`
    /// leaves the proposal pending.
    ///
    /// # Errors
    /// [`EvolutionError::NotFound`], [`EvolutionError::NotReviewable`]
    /// once applied or rejected, the applier's error, I/O and JSON errors.
    pub fn review<E>(
        &self,
        id: &str,
        review: &Review,
        now: i64,
        apply: impl FnOnce(&Proposal) -> std::result::Result<Value, E>,
    ) -> std::result::Result<Proposal, E>
    where
        E: From<EvolutionError>,
    {
        let _g = self.guard();
        let path = self.root.join(PROPOSALS);
        let mut file: ProposalFile = read_json(&path)?;
        let p = file
            .proposals
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or(EvolutionError::NotFound)?;
        if p.status != ProposalStatus::PendingReview {
            return Err(EvolutionError::NotReviewable {
                id: p.id.clone(),
                status: p.status.as_str(),
            }
            .into());
        }
        let (status, extra) = match review {
            Review::Apply => (
                ProposalStatus::Applied,
                json!({ "appliedResult": apply(p)? }),
            ),
            Review::Reject { reason } => (
                ProposalStatus::Rejected,
                reason
                    .as_deref()
                    .map(str::trim)
                    .filter(|r| !r.is_empty())
                    .map_or_else(|| json!({}), |r| json!({ "rejectedReason": r })),
            ),
        };
        p.status = status;
        p.updated_at = now;
        merge(&mut p.metadata, extra);
        let out = p.clone();
        write_json(&path, &file)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]

    use super::*;
    use crate::RunState;
    use crate::store::{TargetMode, Trigger};

    fn routine() -> Routine {
        Routine {
            id: "r1".into(),
            name: "Weekly digest".into(),
            project_id: "p1".into(),
            target_mode: TargetMode::Reuse,
            agent_id: "claude".into(),
            prompt: "Summarise the week".into(),
            schedule: "0 9 * * 1".into(),
            enabled: true,
            next_run_at: None,
            created_at: 1,
            updated_at: 1,
            last_run: None,
        }
    }

    fn run(status: RunState) -> RoutineRun {
        RoutineRun {
            id: "rr1".into(),
            routine_id: "r1".into(),
            trigger: Trigger::Manual,
            status,
            run_id: Some("run9".into()),
            conversation_id: Some("c1".into()),
            project_id: Some("p1".into()),
            error: None,
            started_at: 1,
            ended_at: Some(2),
        }
    }

    #[test]
    fn a_succeeded_run_becomes_a_skill_and_a_memory_proposal() {
        let c = crystallize(
            &routine(),
            &run(RunState::Succeeded),
            "Made **3** cards.",
            7,
        )
        .unwrap();
        assert!(
            c.packet
                .body_markdown
                .contains("## Original Automation Prompt\n\nSummarise the week")
        );
        assert!(
            c.packet
                .body_markdown
                .contains("## Run Summary\n\nMade **3** cards.")
        );
        let [skill, memory] = &c.proposals[..] else {
            panic!()
        };
        assert_eq!(skill.target_kind, TargetKind::Skill);
        assert_eq!(
            skill.target_ref.as_deref(),
            Some("skills/weekly-digest-run/SKILL.md")
        );
        assert!(
            skill
                .patch
                .after
                .starts_with("---\nname: \"Weekly digest run skill\"\n")
        );
        assert_eq!(skill.metadata["routineRunId"], "rr1");
        assert_eq!(memory.target_kind, TargetKind::MemoryNode);
        assert_eq!(memory.patch.after, "Weekly digest run: Made **3** cards.");
        assert!(
            c.proposals
                .iter()
                .all(|p| p.status == ProposalStatus::PendingReview
                    && p.source_packet_ids == [c.packet.id.clone()])
        );
        assert_eq!(c.compression_report.status, "skipped");
    }

    #[test]
    fn only_succeeded_runs_of_the_routine_crystallize() {
        for s in [
            RunState::Failed,
            RunState::Running,
            RunState::Canceled,
            RunState::Skipped,
        ] {
            assert!(crystallize(&routine(), &run(s), "", 1).is_err(), "{s:?}");
        }
        let mut other = run(RunState::Succeeded);
        other.routine_id = "r2".into();
        assert!(crystallize(&routine(), &other, "", 1).is_err());
        let c = crystallize(&routine(), &run(RunState::Succeeded), "  ", 1).unwrap();
        assert!(
            c.packet
                .body_markdown
                .contains("No run summary was recorded")
        );
    }

    #[test]
    fn long_runs_are_compacted_and_memory_fits() {
        let long = "word ".repeat(2_000);
        let c = crystallize(&routine(), &run(RunState::Succeeded), &long, 1).unwrap();
        assert_eq!(c.compression_report.status, "applied");
        assert!(c.compression_report.after_tokens < c.compression_report.before_tokens);
        assert!(c.proposals[0].patch.after.contains("omitted roughly"));
        assert!(c.proposals[1].patch.after.chars().count() <= 1000);
    }

    #[test]
    fn a_proposal_is_reviewed_once() {
        let dir = tempfile::tempdir().unwrap();
        let store = EvolutionStore::new(dir.path());
        let c = crystallize(&routine(), &run(RunState::Succeeded), "ok", 1).unwrap();
        store.save(&c).unwrap();
        assert_eq!(store.packets().unwrap().len(), 1);
        let (skill, memory) = (&c.proposals[0].id, &c.proposals[1].id);
        assert_eq!(
            store
                .proposals(Some(ProposalStatus::PendingReview))
                .unwrap()
                .len(),
            2
        );

        // A failed apply keeps it pending.
        let err = store.review(skill, &Review::Apply, 2, |_| {
            Err::<Value, _>(EvolutionError::Invalid("disk full".into()))
        });
        assert!(err.is_err());
        assert_eq!(
            store.proposal(skill).unwrap().status,
            ProposalStatus::PendingReview
        );

        let mut applied = 0;
        let p = store
            .review(skill, &Review::Apply, 3, |_| {
                applied += 1;
                Ok::<_, EvolutionError>(json!({"skillId": "x"}))
            })
            .unwrap();
        assert_eq!((p.status, p.updated_at), (ProposalStatus::Applied, 3));
        assert_eq!(p.metadata["appliedResult"]["skillId"], "x");
        let again = store.review(skill, &Review::Apply, 4, |_| {
            applied += 1;
            Ok::<_, EvolutionError>(Value::Null)
        });
        assert!(matches!(again, Err(EvolutionError::NotReviewable { .. })));
        assert_eq!(applied, 1, "applied exactly once");

        let reason = Some("Dismissed in Automations".to_owned());
        let p = store
            .review(
                memory,
                &Review::Reject { reason },
                5,
                |_| -> Result<Value> { unreachable!() },
            )
            .unwrap();
        assert_eq!(p.status, ProposalStatus::Rejected);
        assert_eq!(p.metadata["rejectedReason"], "Dismissed in Automations");
        assert!(
            store
                .proposals(Some(ProposalStatus::PendingReview))
                .unwrap()
                .is_empty()
        );
        assert!(matches!(
            store.review("nope", &Review::Apply, 6, |_| Ok::<_, EvolutionError>(
                Value::Null
            )),
            Err(EvolutionError::NotFound)
        ));
    }
}
