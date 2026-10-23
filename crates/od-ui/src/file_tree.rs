//! The files panel as data (Studio files): folder tree rows with collapsed
//! folders, the breadcrumb of the folder in view, search, groups by kind
//! with a page size, and free names for new files. Pure, tested natively.

use od_contracts::FileKind;
use std::collections::{BTreeMap, BTreeSet};

/// Files shown per kind group before "Show more".
pub const GROUP_PAGE: usize = 50;

/// One line of the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Row {
    Folder {
        path: String,
        name: String,
        depth: usize,
        open: bool,
        /// Files inside, at any depth.
        count: usize,
    },
    File {
        path: String,
        name: String,
        depth: usize,
    },
}

impl Row {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Folder { path, .. } | Self::File { path, .. } => path,
        }
    }
}

/// Last segment of a project path.
#[must_use]
pub fn base(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Folder holding `path` (`""` at the project root).
#[must_use]
pub fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(dir, _)| dir)
}

/// `dir/name`, or `name` at the root.
#[must_use]
pub fn join(dir: &str, name: &str) -> String {
    let dir = dir.trim_matches('/');
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// Whether `path` is inside folder `dir` (any depth); everything is
/// inside the root `""`.
#[must_use]
pub fn is_inside(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// Every folder: the listed ones plus those the file paths imply.
fn all_folders(files: &[String], folders: &[String]) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = folders.iter().cloned().collect();
    for f in files {
        let mut dir = parent(f);
        while !dir.is_empty() {
            out.insert(dir.to_owned());
            dir = parent(dir);
        }
    }
    out
}

fn by_name(a: &str, b: &str) -> std::cmp::Ordering {
    base(a)
        .to_lowercase()
        .cmp(&base(b).to_lowercase())
        .then_with(|| a.cmp(b))
}

/// Tree rows under folder `scope` (`""` for the whole project): folders
/// first, then files, by name; a folder's content shows only when it is in
/// `open`. With a `query`, a flat list of the matching files instead
/// (path contains it, any case), so a hit in a closed folder still shows.
#[must_use]
pub fn tree_rows(
    files: &[String],
    folders: &[String],
    scope: &str,
    open: &BTreeSet<String>,
    query: &str,
) -> Vec<Row> {
    let query = query.trim().to_lowercase();
    if !query.is_empty() {
        let mut hits: Vec<&String> = files
            .iter()
            .filter(|f| is_inside(f, scope) && f.to_lowercase().contains(&query))
            .collect();
        hits.sort();
        return hits
            .into_iter()
            .map(|f| Row::File {
                path: f.clone(),
                name: f
                    .strip_prefix(scope)
                    .map_or(f.as_str(), |r| r.trim_start_matches('/'))
                    .to_owned(),
                depth: 0,
            })
            .collect();
    }
    let dirs = all_folders(files, folders);
    let mut children: BTreeMap<&str, (Vec<&str>, Vec<&str>)> = BTreeMap::new();
    for d in &dirs {
        children.entry(parent(d)).or_default().0.push(d);
    }
    for f in files {
        children.entry(parent(f)).or_default().1.push(f);
    }
    let count = |dir: &str| files.iter().filter(|f| is_inside(f, dir)).count();
    subtree(scope, 0, &children, open, &count)
}

/// Rows of folder `dir`, and of its open subfolders, at `depth`.
fn subtree(
    dir: &str,
    depth: usize,
    children: &BTreeMap<&str, (Vec<&str>, Vec<&str>)>,
    open: &BTreeSet<String>,
    count: &dyn Fn(&str) -> usize,
) -> Vec<Row> {
    let mut out = Vec::new();
    let Some((sub, own)) = children.get(dir) else {
        return out;
    };
    let mut sub = sub.clone();
    sub.sort_by(|a, b| by_name(a, b));
    let mut own = own.clone();
    own.sort_by(|a, b| by_name(a, b));
    for d in sub {
        let is_open = open.contains(d);
        out.push(Row::Folder {
            path: d.to_owned(),
            name: base(d).to_owned(),
            depth,
            open: is_open,
            count: count(d),
        });
        if is_open {
            out.extend(subtree(d, depth + 1, children, open, count));
        }
    }
    for f in own {
        out.push(Row::File {
            path: f.to_owned(),
            name: base(f).to_owned(),
            depth,
        });
    }
    out
}

/// Breadcrumb of folder `scope`: (label, folder) from the project root,
/// the root's label being `root`.
#[must_use]
pub fn crumbs(scope: &str, root: &str) -> Vec<(String, String)> {
    let mut out = vec![(root.to_owned(), String::new())];
    let mut acc = String::new();
    for seg in scope.split('/').filter(|s| !s.is_empty()) {
        acc = join(&acc, seg);
        out.push((seg.to_owned(), acc.clone()));
    }
    out
}

/// Folders a file or folder `path` can move to: the root and every
/// folder, minus its current folder and (for a folder) itself and its
/// descendants.
#[must_use]
pub fn move_targets(
    path: &str,
    is_folder: bool,
    files: &[String],
    folders: &[String],
) -> Vec<String> {
    let here = parent(path);
    let mut out: Vec<String> = std::iter::once(String::new())
        .chain(all_folders(files, folders))
        .filter(|d| d != here)
        .filter(|d| !is_folder || (d != path && !is_inside(d, path)))
        .collect();
    out.dedup();
    out
}

/// Kind groups of the "by kind" view, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Group {
    Pages,
    Documents,
    Images,
    Sketches,
    Stylesheets,
    Scripts,
    Media,
    Other,
}

impl Group {
    pub const ALL: [Self; 8] = [
        Self::Pages,
        Self::Documents,
        Self::Images,
        Self::Sketches,
        Self::Stylesheets,
        Self::Scripts,
        Self::Media,
        Self::Other,
    ];

    #[must_use]
    pub fn of(path: &str, kind: FileKind) -> Self {
        let lower = path.to_ascii_lowercase();
        match kind {
            FileKind::Html => Self::Pages,
            FileKind::Sketch => Self::Sketches,
            FileKind::Image => Self::Images,
            FileKind::Video | FileKind::Audio => Self::Media,
            FileKind::Pdf | FileKind::Document | FileKind::Presentation | FileKind::Spreadsheet => {
                Self::Documents
            }
            _ if lower.ends_with(".css") || lower.ends_with(".scss") => Self::Stylesheets,
            _ if is_markdown(&lower) || lower.ends_with(".txt") => Self::Documents,
            FileKind::Code => Self::Scripts,
            _ => Self::Other,
        }
    }
}

/// Files by kind group (empty groups left out); `only` keeps those groups
/// when not empty. Paths sorted within a group.
#[must_use]
pub fn grouped(files: &[(String, FileKind)], only: &BTreeSet<Group>) -> Vec<(Group, Vec<String>)> {
    let mut map: BTreeMap<Group, Vec<String>> = BTreeMap::new();
    for (path, kind) in files {
        let g = Group::of(path, *kind);
        if only.is_empty() || only.contains(&g) {
            map.entry(g).or_default().push(path.clone());
        }
    }
    map.into_iter()
        .map(|(g, mut v)| {
            v.sort_by(|a, b| by_name(a, b));
            (g, v)
        })
        .collect()
}

#[must_use]
pub fn is_markdown(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

/// `dir/stem.ext`, or `dir/stem-2.ext`… when taken in `existing`.
#[must_use]
pub fn free_name(existing: &[String], dir: &str, stem: &str, ext: &str) -> String {
    let taken = |p: &str| existing.iter().any(|e| e.eq_ignore_ascii_case(p));
    let first = join(dir, &format!("{stem}.{ext}"));
    if !taken(&first) {
        return first;
    }
    (2..)
        .map(|n| join(dir, &format!("{stem}-{n}.{ext}")))
        .find(|p| !taken(p))
        .unwrap_or(first)
}

/// A file name from a title: spaces to `-`, path separators dropped.
#[must_use]
pub fn stem_of_title(title: &str) -> String {
    let s: String = title
        .trim()
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .filter(|c| !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        .collect();
    let s = s.trim_matches(['-', '.']).to_owned();
    if s.is_empty() {
        "untitled".to_owned()
    } else {
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| match r {
                Row::Folder {
                    name,
                    depth,
                    open,
                    count,
                    ..
                } => {
                    format!(
                        "{}{}{name}/({count})",
                        "  ".repeat(*depth),
                        if *open { "-" } else { "+" }
                    )
                }
                Row::File { name, depth, .. } => format!("{}{name}", "  ".repeat(*depth)),
            })
            .collect()
    }

    #[test]
    fn folders_first_closed_until_opened() {
        let files = v(&[
            "index.html",
            "assets/logo.png",
            "assets/icons/a.svg",
            "About.md",
        ]);
        let folders = v(&["empty"]);
        let closed = tree_rows(&files, &folders, "", &BTreeSet::new(), "");
        assert_eq!(
            names(&closed),
            ["+assets/(2)", "+empty/(0)", "About.md", "index.html"]
        );
        let open: BTreeSet<String> = v(&["assets", "assets/icons"]).into_iter().collect();
        let rows = tree_rows(&files, &folders, "", &open, "");
        assert_eq!(
            names(&rows),
            [
                "-assets/(2)",
                "  -icons/(1)",
                "    a.svg",
                "  logo.png",
                "+empty/(0)",
                "About.md",
                "index.html"
            ]
        );
        // Inside a folder, depths start over.
        let scoped = tree_rows(&files, &folders, "assets", &BTreeSet::new(), "");
        assert_eq!(names(&scoped), ["+icons/(1)", "logo.png"]);
    }

    #[test]
    fn a_search_lists_matches_from_closed_folders() {
        let files = v(&["index.html", "assets/icons/Arrow.svg", "notes.md"]);
        let rows = tree_rows(&files, &[], "", &BTreeSet::new(), "  arrow ");
        assert_eq!(names(&rows), ["assets/icons/Arrow.svg"]);
        let rows = tree_rows(&files, &[], "assets", &BTreeSet::new(), "svg");
        assert_eq!(names(&rows), ["icons/Arrow.svg"]);
        assert!(tree_rows(&files, &[], "", &BTreeSet::new(), "zzz").is_empty());
    }

    #[test]
    fn crumbs_and_paths() {
        assert_eq!(
            crumbs("a/b", "Project"),
            [
                ("Project".to_owned(), String::new()),
                ("a".to_owned(), "a".to_owned()),
                ("b".to_owned(), "a/b".to_owned())
            ]
        );
        assert_eq!(crumbs("", "P").len(), 1);
        assert_eq!(parent("a/b/c.md"), "a/b");
        assert_eq!(parent("c.md"), "");
        assert_eq!(join("", "x"), "x");
        assert_eq!(join("a/", "x"), "a/x");
        assert!(is_inside("a/b", "a"));
        assert!(!is_inside("ab/c", "a"));
        assert!(!is_inside("a", "a"));
    }

    #[test]
    fn a_folder_never_moves_into_itself() {
        let files = v(&["a/x.md", "a/b/y.md", "c/z.md"]);
        assert_eq!(move_targets("a/x.md", false, &files, &[]), ["", "a/b", "c"]);
        assert_eq!(move_targets("a", true, &files, &v(&["d"])), ["c", "d"]);
    }

    #[test]
    fn groups_by_kind_with_a_filter() {
        let files = vec![
            ("index.html".to_owned(), FileKind::Html),
            ("b.css".to_owned(), FileKind::Code),
            ("a.js".to_owned(), FileKind::Code),
            ("notes.md".to_owned(), FileKind::Text),
            ("logo.png".to_owned(), FileKind::Image),
            ("x.bin".to_owned(), FileKind::Binary),
        ];
        let all = grouped(&files, &BTreeSet::new());
        let groups: Vec<Group> = all.iter().map(|(g, _)| *g).collect();
        assert_eq!(
            groups,
            [
                Group::Pages,
                Group::Documents,
                Group::Images,
                Group::Stylesheets,
                Group::Scripts,
                Group::Other
            ]
        );
        let only: BTreeSet<Group> = [Group::Images].into_iter().collect();
        assert_eq!(grouped(&files, &only), [(Group::Images, v(&["logo.png"]))]);
    }

    #[test]
    fn new_names_do_not_collide() {
        let existing = v(&["docs/Brief.md", "docs/brief-2.md"]);
        assert_eq!(
            free_name(&existing, "docs", "Brief", "md"),
            "docs/Brief-3.md"
        );
        assert_eq!(free_name(&existing, "", "Brief", "md"), "Brief.md");
        assert_eq!(stem_of_title("  Launch plan: v2 "), "Launch-plan-v2");
        assert_eq!(stem_of_title("../"), "untitled");
    }
}
