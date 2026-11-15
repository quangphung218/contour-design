//! Preview comments (`preview_comments`, spec 3.3), scoped to a conversation.

use crate::error::{Result, StoreError};
use crate::files;
use crate::paths::validate_project_path;
use crate::store::Store;
use od_contracts::{
    CommentImage, CommentRect, CommentStatus, CreateCommentRequest, FileKind, MAX_COMMENT_IMAGES,
    PreviewComment, UpdateCommentRequest,
};
use rusqlite::{Connection, OptionalExtension, Row, params};

/// Longest note, element text and HTML hint kept (characters).
const MAX_NOTE: usize = 4000;
const MAX_TEXT: usize = 400;
const MAX_HINT: usize = 400;

const COLUMNS: &str = "id, project_id, conversation_id, file_path, element_id, selector, label, \
     text, position_json, html_hint, slide_index, note, status, created_at, updated_at";

/// Images attached to comments, kept beside `preview_comments` (created
/// on open, so it needs no numbered migration).
const SCHEMA: &str = r"
CREATE TABLE IF NOT EXISTS preview_comment_images (
  comment_id TEXT NOT NULL REFERENCES preview_comments(id) ON DELETE CASCADE,
  position INTEGER NOT NULL,
  path TEXT NOT NULL,
  name TEXT NOT NULL,
  PRIMARY KEY (comment_id, position)
);
";

pub(crate) fn ensure_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(SCHEMA)
}

/// What the comments of `project_id` record by path follows files renamed
/// or moved (with their folder): the file a comment is on (its pin) and
/// the images it was sent with. A rename never lands on a file there, so
/// rows already at a new path were about a file gone outside the app:
/// they go, rather than mix with the ones moved there.
pub(crate) fn renamed(
    conn: &Connection,
    project_id: &str,
    pairs: &[(String, String)],
) -> rusqlite::Result<()> {
    for (from, to) in pairs.iter().filter(|(from, to)| from != to) {
        forget(conn, project_id, to)?;
        conn.execute(
            "UPDATE preview_comments SET file_path = ?3 WHERE project_id = ?1 AND file_path = ?2",
            params![project_id, from, to],
        )?;
        conn.execute(
            "UPDATE preview_comment_images SET path = ?3 WHERE path = ?2 \
             AND comment_id IN (SELECT id FROM preview_comments WHERE project_id = ?1)",
            params![project_id, from, to],
        )?;
    }
    Ok(())
}

/// The file at `path` is gone, or a new one is there: the comments on it
/// go (their images' rows with them), and comments sent with an image at
/// `path` keep their note without it. Nothing a file later at `path`
/// would show as its own.
pub(crate) fn forget(conn: &Connection, project_id: &str, path: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM preview_comments WHERE project_id = ?1 AND file_path = ?2",
        params![project_id, path],
    )?;
    conn.execute(
        "DELETE FROM preview_comment_images WHERE path = ?2 \
         AND comment_id IN (SELECT id FROM preview_comments WHERE project_id = ?1)",
        params![project_id, path],
    )?;
    Ok(())
}

/// [`forget`] for every path under folder `dir`, deleted.
pub(crate) fn forget_folder(
    conn: &Connection,
    project_id: &str,
    dir: &str,
) -> rusqlite::Result<()> {
    let prefix = format!("{}/", dir.trim_end_matches('/'));
    // `length` of SQLite itself: `substr` counts characters, Rust's `len`
    // bytes (`Ảnh/` is 4 of one, 6 of the other).
    conn.execute(
        "DELETE FROM preview_comments WHERE project_id = ?1 \
         AND substr(file_path, 1, length(?2)) = ?2",
        params![project_id, prefix],
    )?;
    conn.execute(
        "DELETE FROM preview_comment_images WHERE substr(path, 1, length(?2)) = ?2 \
         AND comment_id IN (SELECT id FROM preview_comments WHERE project_id = ?1)",
        params![project_id, prefix],
    )?;
    Ok(())
}

/// Every comment's images, in order (one query for the whole list).
fn attach_images(conn: &Connection, comments: &mut [PreviewComment]) -> Result<()> {
    if comments.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "SELECT comment_id, path, name FROM preview_comment_images \
         WHERE comment_id IN (SELECT id FROM preview_comments WHERE conversation_id = ?1) \
         ORDER BY comment_id, position",
    )?;
    let rows = stmt.query_map([&comments[0].conversation_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            CommentImage {
                path: r.get(1)?,
                name: r.get(2)?,
            },
        ))
    })?;
    for row in rows {
        let (id, image) = row?;
        if let Some(c) = comments.iter_mut().find(|c| c.id == id) {
            c.attachments.push(image);
        }
    }
    Ok(())
}

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
        attachments: Vec::new(),
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
            let mut comments = rows.collect::<rusqlite::Result<Vec<_>>>()?;
            attach_images(conn, &mut comments)?;
            Ok(comments)
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
        let attachments = self.comment_images(project_id, &req.attachments)?;
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
            attachments,
            created_at: now,
            updated_at: now,
        };
        let position = serde_json::to_string(&comment.position)?;
        let images: Vec<String> = comment.attachments.iter().map(|a| a.path.clone()).collect();
        let root = if images.is_empty() {
            None
        } else {
            Some(self.root_for(project_id)?)
        };
        self.with_conn(|conn| {
            let conn = conn.unchecked_transaction()?;
            // Its images are the comment's now: no release takes them.
            // One a release took since they were checked is gone: the
            // comment is refused rather than kept pointing to nothing.
            crate::drafts::claim(&conn, project_id, &images)?;
            if let Some(root) = &root {
                for path in &images {
                    files::stat_file(root, path)?;
                }
            }
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
            for (i, image) in comment.attachments.iter().enumerate() {
                conn.execute(
                    "INSERT INTO preview_comment_images (comment_id, position, path, name) \
                     VALUES (?1, ?2, ?3, ?4)",
                    params![comment.id, i as i64, image.path, image.name],
                )?;
            }
            conn.commit()?;
            Ok(())
        })?;
        Ok(comment)
    }

    /// Checks the images a new comment attaches: at most
    /// [`MAX_COMMENT_IMAGES`], each an image file of the project.
    fn comment_images(&self, project_id: &str, paths: &[String]) -> Result<Vec<CommentImage>> {
        if paths.len() > MAX_COMMENT_IMAGES {
            return Err(StoreError::BadRequest(format!(
                "at most {MAX_COMMENT_IMAGES} images per comment"
            )));
        }
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        let root = self.root_for(project_id)?;
        paths
            .iter()
            .map(|raw| {
                let path = validate_project_path(raw)?;
                let (file, _) = files::stat_file(&root, &path)?;
                if file.kind != FileKind::Image {
                    return Err(StoreError::BadRequest(format!("{path} is not an image")));
                }
                Ok(CommentImage {
                    name: file
                        .name
                        .rsplit('/')
                        .next()
                        .unwrap_or(&file.name)
                        .to_owned(),
                    path: file.path,
                })
            })
            .collect()
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
            attach_images(conn, std::slice::from_mut(&mut comment))?;
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
    fn images_attach_in_order_and_must_be_project_images() {
        let (_tmp, s, p, c) = store();
        for name in ["shots/a.png", "shots/b.jpg", "notes.txt"] {
            s.write_file(&p, name, b"x", true, None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        let with = |paths: &[&str]| CreateCommentRequest {
            attachments: paths.iter().map(|p| (*p).to_owned()).collect(),
            ..req("See the shot")
        };
        let made = s
            .create_comment(&p, &c, with(&["shots/b.jpg", "shots/a.png"]))
            .unwrap_or_else(|e| panic!("{e}"));
        let names: Vec<_> = made.attachments.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["b.jpg", "a.png"]);
        let plain = s
            .create_comment(&p, &c, req("No image"))
            .unwrap_or_else(|e| panic!("{e}"));
        let listed = s.list_comments(&p, &c).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(listed, vec![made.clone(), plain]);
        // An update keeps them; a delete takes them along.
        let updated = s
            .update_comment(
                &p,
                &c,
                &made.id,
                UpdateCommentRequest {
                    note: Some("Still".into()),
                    status: None,
                },
            )
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(updated.attachments, made.attachments);
        s.delete_comment(&p, &c, &made.id)
            .unwrap_or_else(|e| panic!("{e}"));
        let left: i64 = s
            .with_conn(|conn| {
                Ok(
                    conn.query_row("SELECT COUNT(*) FROM preview_comment_images", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(left, 0);
        // Not an image, not there, outside the project, too many.
        for bad in [
            vec!["notes.txt"],
            vec!["shots/missing.png"],
            vec!["../x.png"],
            vec!["shots/a.png"; MAX_COMMENT_IMAGES + 1],
        ] {
            assert!(s.create_comment(&p, &c, with(&bad)).is_err(), "{bad:?}");
        }
        assert_eq!(
            s.list_comments(&p, &c).map(|l| l.len()).ok(),
            Some(1),
            "a refused comment is not stored"
        );
    }

    /// A comment on `file`, sent with `images`.
    fn on(s: &Store, p: &str, c: &str, file: &str, images: &[&str]) -> PreviewComment {
        s.create_comment(
            p,
            c,
            CreateCommentRequest {
                file_path: file.into(),
                attachments: images.iter().map(|i| (*i).to_owned()).collect(),
                ..req(&format!("On {file}"))
            },
        )
        .unwrap_or_else(|e| panic!("{e}"))
    }

    fn write(s: &Store, p: &str, files: &[&str]) {
        for f in files {
            s.write_file(p, f, b"x", true, None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
    }

    /// Each comment: its file and its images' paths.
    fn where_(s: &Store, p: &str, c: &str) -> Vec<(String, Vec<String>)> {
        s.list_comments(p, c)
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .map(|c| {
                (
                    c.file_path,
                    c.attachments.into_iter().map(|a| a.path).collect(),
                )
            })
            .collect()
    }

    fn pair(file: &str, images: &[&str]) -> (String, Vec<String>) {
        (
            file.to_owned(),
            images.iter().map(|i| (*i).to_owned()).collect(),
        )
    }

    /// Deleted outside the app (the agent's shell, Finder).
    fn remove_outside(s: &Store, p: &str, file: &str) {
        let full = s.stat_file(p, file).unwrap_or_else(|e| panic!("{e}")).1;
        std::fs::remove_file(full).unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn a_rename_takes_the_comments_pins_and_images_along() {
        let (_tmp, s, p, c) = store();
        write(&s, &p, &["a.html", "comments/pic.png"]);
        on(&s, &p, &c, "a.html", &["comments/pic.png"]);
        s.rename_file(&p, "a.html", "refs/a.html")
            .unwrap_or_else(|e| panic!("{e}"));
        s.rename_file(&p, "comments/pic.png", "comments/reference.png")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            where_(&s, &p, &c),
            [pair("refs/a.html", &["comments/reference.png"])],
            "QA-099: the image follows its file"
        );
        // Renamed onto itself, nothing moves and nothing goes.
        s.rename_file(&p, "refs/a.html", "refs/a.html")
            .unwrap_or_else(|e| panic!("{e}"));
        s.rename_file(&p, "comments/reference.png", "comments/reference.png")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            where_(&s, &p, &c),
            [pair("refs/a.html", &["comments/reference.png"])]
        );
    }

    #[test]
    fn a_folder_moved_takes_the_comments_on_its_files_and_their_images() {
        let (_tmp, s, p, c) = store();
        write(
            &s,
            &p,
            &[
                "pages/a.html",
                "pages2/b.html",
                "comments/pic.png",
                "old/z.html",
            ],
        );
        on(&s, &p, &c, "pages/a.html", &["comments/pic.png"]);
        on(&s, &p, &c, "pages2/b.html", &[]);
        s.move_folder(&p, "comments", "old/comments")
            .unwrap_or_else(|e| panic!("{e}"));
        s.move_folder(&p, "pages", "site/pages")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            where_(&s, &p, &c),
            [
                pair("site/pages/a.html", &["old/comments/pic.png"]),
                pair("pages2/b.html", &[]),
            ],
            "QA-099: a folder's prefix, and only its own"
        );
    }

    #[test]
    fn a_deleted_files_comments_do_not_come_back_on_the_next_file_at_its_name() {
        let (_tmp, s, p, c) = store();
        write(&s, &p, &["a.html", "b.html", "comments/pic.png"]);
        on(&s, &p, &c, "a.html", &["comments/pic.png"]);
        s.delete_file(&p, "a.html")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(where_(&s, &p, &c), [], "QA-100: gone with its file");
        s.rename_file(&p, "b.html", "a.html")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(where_(&s, &p, &c), []);
        let images: i64 = s
            .with_conn(|conn| {
                Ok(
                    conn.query_row("SELECT COUNT(*) FROM preview_comment_images", [], |r| {
                        r.get(0)
                    })?,
                )
            })
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(images, 0, "the image rows went with the comment");
    }

    #[test]
    fn a_deleted_image_leaves_its_comment_without_it() {
        let (_tmp, s, p, c) = store();
        write(&s, &p, &["a.html", "comments/one.png", "comments/two.png"]);
        on(
            &s,
            &p,
            &c,
            "a.html",
            &["comments/one.png", "comments/two.png"],
        );
        s.delete_file(&p, "comments/one.png")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(where_(&s, &p, &c), [pair("a.html", &["comments/two.png"])]);
        // Another image put at that name later is not the comment's.
        write(&s, &p, &["comments/one.png"]);
        assert_eq!(where_(&s, &p, &c), [pair("a.html", &["comments/two.png"])]);
    }

    #[test]
    fn a_deleted_folder_takes_the_comments_on_its_files_and_its_images() {
        let (_tmp, s, p, c) = store();
        write(
            &s,
            &p,
            &["pages/a.html", "pages2/b.html", "comments/pic.png"],
        );
        on(&s, &p, &c, "pages/a.html", &[]);
        on(&s, &p, &c, "pages2/b.html", &["comments/pic.png"]);
        s.delete_folder(&p, "pages")
            .unwrap_or_else(|e| panic!("{e}"));
        s.delete_folder(&p, "comments")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            where_(&s, &p, &c),
            [pair("pages2/b.html", &[])],
            "only the folder's own, not pages2/"
        );
        write(&s, &p, &["pages/a.html"]);
        assert_eq!(where_(&s, &p, &c), [pair("pages2/b.html", &[])]);
    }

    #[test]
    fn a_rename_onto_comments_left_by_a_file_gone_outside_does_not_mix_them() {
        let (_tmp, s, p, c) = store();
        write(&s, &p, &["a.html", "b.html"]);
        on(&s, &p, &c, "a.html", &[]);
        on(&s, &p, &c, "b.html", &[]);
        remove_outside(&s, &p, "a.html");
        s.rename_file(&p, "b.html", "a.html")
            .unwrap_or_else(|e| panic!("{e}"));
        let left = s.list_comments(&p, &c).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(left.len(), 1, "only b's comment, moved: {left:?}");
        assert_eq!(left[0].note, "On b.html");
        assert_eq!(left[0].file_path, "a.html");
    }

    #[test]
    fn a_new_file_where_one_went_outside_the_app_starts_with_no_comments() {
        let (_tmp, s, p, c) = store();
        write(&s, &p, &["w.html", "x.html", "u.html", "kept.html"]);
        for f in ["w.html", "x.html", "u.html", "kept.html"] {
            on(&s, &p, &c, f, &[]);
        }
        for f in ["w.html", "x.html", "u.html"] {
            remove_outside(&s, &p, f);
        }
        s.write_file(&p, "w.html", b"<p>new</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        s.create_file_exact(&p, "x.html", b"<p>new</p>")
            .unwrap_or_else(|e| panic!("{e}"));
        s.upload_file(&p, None, "u.html", b"<p>new</p>")
            .unwrap_or_else(|e| panic!("{e}"));
        // A file written over (the agent's edit, a save) keeps its comments.
        s.write_file(&p, "kept.html", b"<p>edited</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        s.replace_file(&p, "kept.html", b"<p>again</p>", "Manual edit")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(where_(&s, &p, &c), [pair("kept.html", &[])]);
    }

    #[test]
    fn the_entry_file_follows_a_rename_and_a_folder_move() {
        let (_tmp, s, p, _) = store();
        write(&s, &p, &["site/home.html"]);
        let mut project = s.get_project(&p).unwrap_or_else(|e| panic!("{e}"));
        project.metadata = Some(od_contracts::ProjectMetadata {
            entry_file: Some("site/home.html".into()),
            intent: Some("keep".into()),
            ..Default::default()
        });
        s.with_conn(|conn| crate::db::update_project(conn, &project))
            .unwrap_or_else(|e| panic!("{e}"));
        let entry = |s: &Store| {
            let m = s.get_project(&p).unwrap_or_else(|e| panic!("{e}")).metadata;
            m.map(|m| (m.entry_file, m.intent))
        };
        s.rename_file(&p, "site/home.html", "site/index.html")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            entry(&s),
            Some((Some("site/index.html".into()), Some("keep".into())))
        );
        s.move_folder(&p, "site", "web")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            entry(&s),
            Some((Some("web/index.html".into()), Some("keep".into())))
        );
    }

    fn set_entry(s: &Store, p: &str, entry: &str) {
        let mut project = s.get_project(p).unwrap_or_else(|e| panic!("{e}"));
        project.metadata = Some(od_contracts::ProjectMetadata {
            entry_file: Some(entry.into()),
            intent: Some("keep".into()),
            ..Default::default()
        });
        s.with_conn(|conn| crate::db::update_project(conn, &project))
            .unwrap_or_else(|e| panic!("{e}"));
    }

    fn entry(s: &Store, p: &str) -> Option<(Option<String>, Option<String>)> {
        let m = s.get_project(p).unwrap_or_else(|e| panic!("{e}")).metadata;
        m.map(|m| (m.entry_file, m.intent))
    }

    fn on_disk(s: &Store, p: &str) -> Vec<String> {
        let project = s.get_project(p).unwrap_or_else(|e| panic!("{e}"));
        let root = s.project_root(&project).unwrap_or_else(|e| panic!("{e}"));
        let mut names: Vec<String> = crate::files::list_files(&root)
            .unwrap_or_else(|e| panic!("{e}"))
            .into_iter()
            .map(|f| f.path)
            .collect();
        names.sort();
        names
    }

    /// A folder name with diacritics: more bytes than characters.
    #[test]
    fn a_deleted_folder_with_diacritics_takes_its_comments_images_and_entry() {
        let (_tmp, s, p, c) = store();
        write(&s, &p, &["Ảnh/home.html", "Ảnh/pic.png", "Ảnhx.html"]);
        on(&s, &p, &c, "Ảnh/home.html", &[]);
        on(&s, &p, &c, "Ảnhx.html", &["Ảnh/pic.png"]);
        set_entry(&s, &p, "Ảnh/home.html");
        s.delete_folder(&p, "Ảnh").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(where_(&s, &p, &c), [pair("Ảnhx.html", &[])]);
        assert_eq!(entry(&s, &p), Some((None, Some("keep".into()))));
    }

    /// A page deleted, then made again outside the app (the agent's
    /// shell) and renamed away and back: the deleted page's history stays
    /// closed, not the new page's.
    #[test]
    fn a_deleted_pages_history_does_not_come_back_through_a_round_trip() {
        let (_tmp, s, p, _) = store();
        for t in [&b"<p>old one</p>"[..], b"<p>old two</p>"] {
            s.write_file(&p, "page.html", t, true, None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        s.delete_file(&p, "page.html")
            .unwrap_or_else(|e| panic!("{e}"));
        let dir = s
            .project_root(&s.get_project(&p).unwrap_or_else(|e| panic!("{e}")))
            .unwrap_or_else(|e| panic!("{e}"))
            .dir;
        std::fs::write(dir.join("page.html"), b"<p>agent new</p>")
            .unwrap_or_else(|e| panic!("{e}"));
        s.rename_file(&p, "page.html", "page.md")
            .unwrap_or_else(|e| panic!("{e}"));
        s.rename_file(&p, "page.md", "page.html")
            .unwrap_or_else(|e| panic!("{e}"));
        s.write_file(&p, "page.html", b"<p>new save</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        let list = s
            .list_versions(&p, "page.html")
            .unwrap_or_else(|e| panic!("{e}"))
            .versions;
        assert_eq!(
            list.len(),
            1,
            "the deleted page's history came back: {list:?}"
        );
    }

    #[test]
    fn a_deleted_entry_file_no_longer_names_the_entry_page() {
        let (_tmp, s, p, _) = store();
        write(
            &s,
            &p,
            &["site/home.html", "x/a.html", "xy.html", "gone.html"],
        );
        set_entry(&s, &p, "site/home.html");
        s.delete_file(&p, "site/home.html")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(entry(&s, &p), Some((None, Some("keep".into()))));
        // A file put at that name later is not the entry page for it.
        write(&s, &p, &["site/home.html"]);
        assert_eq!(entry(&s, &p), Some((None, Some("keep".into()))));
        // A folder's own files only, not `xy.html`.
        set_entry(&s, &p, "xy.html");
        s.delete_folder(&p, "x").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            entry(&s, &p),
            Some((Some("xy.html".into()), Some("keep".into())))
        );
        write(&s, &p, &["x/a.html"]);
        set_entry(&s, &p, "x/a.html");
        s.delete_folder(&p, "x").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(entry(&s, &p), Some((None, Some("keep".into()))));
        // Gone outside the app, then deleted in it.
        set_entry(&s, &p, "gone.html");
        remove_outside(&s, &p, "gone.html");
        assert!(matches!(
            s.delete_file(&p, "gone.html"),
            Err(StoreError::FileNotFound)
        ));
        assert_eq!(entry(&s, &p), Some((None, Some("keep".into()))));
    }

    #[test]
    fn a_page_deleted_after_it_went_outside_the_app_closes_its_history() {
        let (_tmp, s, p, _) = store();
        s.write_file(&p, "gone.html", b"<p>one</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        remove_outside(&s, &p, "gone.html");
        assert!(matches!(
            s.delete_file(&p, "gone.html"),
            Err(StoreError::FileNotFound)
        ));
        s.write_file(&p, "gone.html", b"<p>new</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        let list = s
            .list_versions(&p, "gone.html")
            .unwrap_or_else(|e| panic!("{e}"))
            .versions;
        assert_eq!(list.len(), 1, "the next page there took the old history");
    }

    #[test]
    fn a_page_renamed_to_markdown_and_back_keeps_its_history() {
        let (_tmp, s, p, _) = store();
        for text in [&b"<p>one</p>"[..], b"<p>two</p>"] {
            s.write_file(&p, "page.html", text, true, None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        s.rename_file(&p, "page.html", "page.md")
            .unwrap_or_else(|e| panic!("{e}"));
        s.rename_file(&p, "page.md", "page.html")
            .unwrap_or_else(|e| panic!("{e}"));
        s.write_file(&p, "page.html", b"<p>three</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        let list = s
            .list_versions(&p, "page.html")
            .unwrap_or_else(|e| panic!("{e}"))
            .versions;
        assert_eq!(list.len(), 3, "the round trip lost the history: {list:?}");
        // A markdown file put where the page went outside the app is not
        // the page: renamed to one, it starts its own history.
        s.rename_file(&p, "page.html", "page.md")
            .unwrap_or_else(|e| panic!("{e}"));
        remove_outside(&s, &p, "page.md");
        s.upload_file(&p, None, "page.md", b"# new")
            .unwrap_or_else(|e| panic!("{e}"));
        s.rename_file(&p, "page.md", "page.html")
            .unwrap_or_else(|e| panic!("{e}"));
        let list = s
            .list_versions(&p, "page.html")
            .unwrap_or_else(|e| panic!("{e}"))
            .versions;
        assert_eq!(list.len(), 1);
        // Nor does one put where a folder holding such a file was deleted.
        s.write_file(&p, "page.html", b"<p>four</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        s.rename_file(&p, "page.html", "x/page.md")
            .unwrap_or_else(|e| panic!("{e}"));
        s.delete_folder(&p, "x").unwrap_or_else(|e| panic!("{e}"));
        // Put back outside the app (the agent's shell).
        let dir = s
            .project_root(&s.get_project(&p).unwrap_or_else(|e| panic!("{e}")))
            .unwrap_or_else(|e| panic!("{e}"))
            .dir
            .join("x");
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(dir.join("page.md"), b"# again").unwrap_or_else(|e| panic!("{e}"));
        s.rename_file(&p, "x/page.md", "page.html")
            .unwrap_or_else(|e| panic!("{e}"));
        let list = s
            .list_versions(&p, "page.html")
            .unwrap_or_else(|e| panic!("{e}"))
            .versions;
        assert_eq!(list.len(), 1, "the folder's page.md took the old history");
    }

    /// The page renamed to markdown, then that file replaced outside the
    /// app (the agent's shell: removed, another written): renamed to a
    /// page, the other file does not take the old page's history.
    #[test]
    fn a_markdown_file_replaced_outside_the_app_does_not_take_the_pages_history() {
        let (_tmp, s, p, _) = store();
        for text in [&b"<p>one</p>"[..], b"<p>two</p>"] {
            s.write_file(&p, "page.html", text, true, None)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        s.rename_file(&p, "page.html", "page.md")
            .unwrap_or_else(|e| panic!("{e}"));
        let full = s
            .stat_file(&p, "page.md")
            .unwrap_or_else(|e| panic!("{e}"))
            .1;
        std::fs::remove_file(&full).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&full, b"# agent notes").unwrap_or_else(|e| panic!("{e}"));
        s.rename_file(&p, "page.md", "page.html")
            .unwrap_or_else(|e| panic!("{e}"));
        s.write_file(&p, "page.html", b"<p>agent page</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        let list = s
            .list_versions(&p, "page.html")
            .unwrap_or_else(|e| panic!("{e}"))
            .versions;
        assert_eq!(list.len(), 1, "the old page's history came back: {list:?}");
    }

    /// The sidecar cannot take the new name: the file stays at its old
    /// name, and so do its comments and history (all or nothing).
    #[test]
    fn a_rename_whose_sidecar_cannot_follow_leaves_everything_at_the_old_name() {
        let (_tmp, s, p, c) = store();
        let manifest = serde_json::json!({"kind": "page"});
        s.write_file(&p, "a.html", b"<p>a</p>", true, Some(&manifest))
            .unwrap_or_else(|e| panic!("{e}"));
        on(&s, &p, &c, "a.html", &[]);
        let dir = s
            .project_root(&s.get_project(&p).unwrap_or_else(|e| panic!("{e}")))
            .unwrap_or_else(|e| panic!("{e}"))
            .dir;
        std::fs::create_dir_all(dir.join("b.html.artifact.json/x"))
            .unwrap_or_else(|e| panic!("{e}"));
        assert!(s.rename_file(&p, "a.html", "b.html").is_err());
        assert!(s.stat_file(&p, "a.html").is_ok() && s.stat_file(&p, "b.html").is_err());
        assert_eq!(where_(&s, &p, &c), vec![pair("a.html", &[])]);
        let len = |name: &str| {
            s.list_versions(&p, name)
                .map(|r| r.versions.len())
                .unwrap_or(0)
        };
        assert_eq!((len("a.html"), len("b.html")), (1, 0));
    }

    #[test]
    fn a_rename_with_project_metadata_that_is_not_json_still_takes_the_comments() {
        let (_tmp, s, p, c) = store();
        write(&s, &p, &["a.html"]);
        on(&s, &p, &c, "a.html", &[]);
        for (i, bad) in ["not json", ""].into_iter().enumerate() {
            s.with_conn(|conn| {
                conn.execute(
                    "UPDATE projects SET metadata_json = ?2 WHERE id = ?1",
                    rusqlite::params![p, bad],
                )?;
                Ok(())
            })
            .unwrap_or_else(|e| panic!("{e}"));
            let (from, to) = if i == 0 {
                ("a.html", "b.html")
            } else {
                ("b.html", "a.html")
            };
            s.rename_file(&p, from, to)
                .unwrap_or_else(|e| panic!("{bad:?}: {e}"));
            assert_eq!(where_(&s, &p, &c), [pair(to, &[])], "{bad:?}");
        }
        // Deleted, the entry page it is not cleared from does not fail it.
        s.delete_file(&p, "a.html")
            .unwrap_or_else(|e| panic!("{e}"));
        write(&s, &p, &["x/y.html"]);
        s.delete_folder(&p, "x").unwrap_or_else(|e| panic!("{e}"));
    }

    #[test]
    fn a_rename_only_in_case_renames_the_file_on_disk_too() {
        let (_tmp, s, p, c) = store();
        write(&s, &p, &["a.html", "comments/pic.png"]);
        on(&s, &p, &c, "a.html", &["comments/pic.png"]);
        let file = s
            .rename_file(&p, "a.html", "A.html")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(file.path, "A.html");
        s.rename_file(&p, "comments/pic.png", "comments/Pic.png")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(on_disk(&s, &p), ["A.html", "comments/Pic.png"]);
        assert_eq!(where_(&s, &p, &c), [pair("A.html", &["comments/Pic.png"])]);
    }

    #[cfg(unix)]
    #[test]
    fn a_rename_whose_history_cannot_move_leaves_all_at_the_old_name() {
        use std::os::unix::fs::PermissionsExt;
        let (_tmp, s, p, c) = store();
        s.write_file(&p, "a.html", b"<p>a</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        on(&s, &p, &c, "a.html", &[]);
        let project = s.get_project(&p).unwrap_or_else(|e| panic!("{e}"));
        let histories = s
            .project_root(&project)
            .unwrap_or_else(|e| panic!("{e}"))
            .dir
            .join(crate::versions::VERSION_ROOT);
        let mode = |m| std::fs::set_permissions(&histories, std::fs::Permissions::from_mode(m));
        mode(0o555).unwrap_or_else(|e| panic!("{e}"));
        let res = s.rename_file(&p, "a.html", "b.html");
        mode(0o755).unwrap_or_else(|e| panic!("{e}"));
        assert!(res.is_err(), "{res:?}");
        assert_eq!(on_disk(&s, &p), ["a.html"], "the file is back at its name");
        assert_eq!(where_(&s, &p, &c), [pair("a.html", &[])]);
        let versions = s
            .list_versions(&p, "a.html")
            .unwrap_or_else(|e| panic!("{e}"))
            .versions;
        assert_eq!(versions.len(), 1);
    }

    #[test]
    fn a_rename_or_move_whose_records_cannot_follow_puts_the_files_back() {
        let (_tmp, s, p, c) = store();
        s.write_file(&p, "a.html", b"<p>a</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        s.write_file(&p, "pages/b.html", b"<p>b</p>", true, None)
            .unwrap_or_else(|e| panic!("{e}"));
        on(&s, &p, &c, "a.html", &[]);
        on(&s, &p, &c, "pages/b.html", &[]);
        s.with_conn(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER no_moves BEFORE UPDATE ON preview_comments \
                 BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
            )?;
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert!(s.rename_file(&p, "a.html", "b.html").is_err());
        assert!(s.move_folder(&p, "pages", "site/pages").is_err());
        assert_eq!(on_disk(&s, &p), ["a.html", "pages/b.html"]);
        assert_eq!(
            where_(&s, &p, &c),
            [pair("a.html", &[]), pair("pages/b.html", &[])]
        );
        for (name, len) in [("a.html", 1), ("pages/b.html", 1), ("b.html", 0)] {
            let list = crate::versions::list_versions(
                &s.project_root(&s.get_project(&p).unwrap_or_else(|e| panic!("{e}")))
                    .unwrap_or_else(|e| panic!("{e}"))
                    .dir,
                name,
            )
            .unwrap_or_else(|e| panic!("{e}"));
            assert_eq!(list.len(), len, "{name}: the history moved anyway");
        }
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
