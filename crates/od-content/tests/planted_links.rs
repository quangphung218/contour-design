//! An agent that can write `<data>/skills` (or `~/.claude/skills`) plants
//! a link to a file of the user's inside the folder Contour is building,
//! while Contour builds it: nothing Contour copies is ever written through
//! it (`crates/od-server/CLAUDE.md`, invariant 16). Each write that builds
//! a folder from a tree (install, install over a user skill, replace from
//! source, an override's first copy, a design system generated, Copy to
//! Claude Code) is raced by a thread planting links, 10 times.

#![cfg(unix)]
#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::design_system_gen::{self, GenerateRequest};
use od_content::user_content::{self, ExportFiles, SkillDraft, WriteError};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

const THE_USERS: &str = "the user's own file";

/// Runs `plant` over and over on another thread until the returned guard
/// is dropped.
struct Planter {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Planter {
    fn start(plant: impl Fn() + Send + 'static) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let done = Arc::clone(&stop);
        let thread = std::thread::spawn(move || {
            while !done.load(Ordering::Relaxed) {
                plant();
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for Planter {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            t.join().unwrap();
        }
    }
}

/// Plants a link to `victim` named each of `names` in every folder of
/// `dirs` whose name `pick` takes (a staging folder seen by listing, as an
/// agent watching the folder would), and in `dirs` themselves when `pick`
/// takes `""`.
fn plant_in_new_folders(
    dirs: Vec<PathBuf>,
    pick: impl Fn(&str) -> bool + Send + 'static,
    names: &'static [&'static str],
    victim: PathBuf,
) -> impl Fn() + Send + 'static {
    move || {
        for dir in &dirs {
            let Ok(list) = std::fs::read_dir(dir) else {
                continue;
            };
            for entry in list.flatten() {
                if !pick(&entry.file_name().to_string_lossy()) {
                    continue;
                }
                for name in names {
                    let _ = std::os::unix::fs::symlink(&victim, entry.path().join(name));
                }
            }
        }
    }
}

/// A source skill `tea` in `dir`: its `SKILL.md` with `marker`, and many
/// side files (a wider window for the planter).
fn source_skill(dir: &Path, marker: &str) {
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: tea\ndescription: d\n---\n\n{marker}\n"),
    )
    .unwrap();
    for i in 0..300 {
        std::fs::write(dir.join(format!("assets/a{i:03}.txt")), marker).unwrap();
    }
}

fn victim(tmp: &Path) -> PathBuf {
    let v = tmp.join("victim.txt");
    std::fs::write(&v, THE_USERS).unwrap();
    v
}

fn assert_untouched(victim: &Path, what: &str, i: usize) {
    let now = std::fs::read_to_string(victim).unwrap();
    assert_eq!(
        now, THE_USERS,
        "{what} wrote through a planted link (try {i})"
    );
}

/// Reviewer probe (2026-11-27): a fresh install builds in
/// `.installing-tea-<pid>`; an agent makes that folder first, with a link
/// at `SKILL.md`. `copy_dir` took the folder as it was (`create_dir_all`)
/// and wrote through the link (`std::fs::write`), 10 of 10 times.
#[test]
fn an_install_never_writes_through_a_link_planted_in_its_staging_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("skills");
    std::fs::create_dir_all(&user).unwrap();
    let src = tmp.path().join("src/tea");
    source_skill(&src, "FROM THE SOURCE");
    // A large source: read before the folder is made, a wide window.
    std::fs::write(src.join("big.bin"), vec![b'x'; 16 * 1024 * 1024]).unwrap();
    let victim = victim(tmp.path());
    // At the name a pid gives (what main used), and in any staging folder
    // seen by listing (whatever its name).
    let guessed = user.join(format!(".installing-tea-{}", std::process::id()));
    let (v, g) = (victim.clone(), guessed.clone());
    let _guess = Planter::start(move || {
        let _ = std::fs::create_dir(&g);
        let _ = std::os::unix::fs::symlink(&v, g.join("SKILL.md"));
    });
    let _watch = Planter::start(plant_in_new_folders(
        vec![user.clone()],
        |n| n.starts_with(".installing-"),
        &["SKILL.md"],
        victim.clone(),
    ));
    for i in 0..10 {
        let _ = user_content::install_skill(&src, &user, |_| false);
        assert_untouched(&victim, "install", i);
        let _ = std::fs::remove_dir_all(user.join("tea"));
    }
}

/// Install over a user skill `tea` builds in `.previous/.incoming-tea-…`.
#[test]
fn an_install_over_a_user_skill_never_writes_through_a_planted_link() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("skills");
    source_skill(&user.join("tea"), "mine");
    let src = tmp.path().join("src/tea");
    source_skill(&src, "FROM THE SOURCE");
    let victim = victim(tmp.path());
    std::fs::create_dir_all(user.join(".previous")).unwrap();
    let guessed = user.join(format!(".previous/.incoming-tea-{}", std::process::id()));
    let (v, g) = (victim.clone(), guessed.clone());
    let _guess = Planter::start(move || {
        let _ = std::fs::create_dir(&g);
        let _ = std::os::unix::fs::symlink(&v, g.join("SKILL.md"));
    });
    let _watch = Planter::start(plant_in_new_folders(
        vec![user.join(".previous")],
        |n| n.starts_with(".incoming-"),
        &["SKILL.md"],
        victim.clone(),
    ));
    for i in 0..10 {
        let _ = user_content::install_skill(&src, &user, |_| false);
        assert_untouched(&victim, "install over a user skill", i);
    }
}

/// Reviewer probe (2026-11-27): "Replace from source" builds in
/// `.previous/.incoming-tea-<pid>` (made with `create_dir`), then wrote
/// with `std::fs::write`: a link planted inside right after was followed,
/// 10 of 10 times.
#[test]
fn a_replace_never_writes_through_a_link_planted_in_incoming() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("skills");
    source_skill(&user.join("tea"), "mine");
    let src = tmp.path().join("agent/tea");
    source_skill(&src, "FROM THE SOURCE");
    let victim = victim(tmp.path());
    std::fs::create_dir_all(user.join(".previous")).unwrap();
    let guessed = user.join(format!(".previous/.incoming-tea-{}", std::process::id()));
    let (v, g) = (victim.clone(), guessed.clone());
    let _guess = Planter::start(move || {
        let _ = std::os::unix::fs::symlink(&v, g.join("SKILL.md"));
    });
    let _watch = Planter::start(plant_in_new_folders(
        vec![user.join(".previous")],
        |n| n.starts_with(".incoming-"),
        &["SKILL.md"],
        victim.clone(),
    ));
    let hash = ExportFiles::read_copy(&src).unwrap().hash();
    for i in 0..10 {
        let _ = user_content::replace_skill_from_source(&src, &user, "tea", &hash);
        assert_untouched(&victim, "replace from source", i);
    }
}

/// The first Save of a bundled skill copies it as an override
/// `<user>/tea`: a link planted in that folder while it is filled is
/// never written through.
#[test]
fn an_overrides_first_copy_never_writes_through_a_planted_link() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("skills");
    std::fs::create_dir_all(&user).unwrap();
    let bundled = tmp.path().join("bundled/tea");
    source_skill(&bundled, "BUNDLED");
    let victim = victim(tmp.path());
    let _watch = Planter::start(plant_in_new_folders(
        vec![user.clone()],
        |n| n == "tea",
        &["SKILL.md", "assets"],
        victim.clone(),
    ));
    let draft = SkillDraft {
        name: "tea".into(),
        description: "d".into(),
        triggers: vec![],
        body: "edited".into(),
    };
    for i in 0..10 {
        let _ = user_content::save_skill(&user, "tea", &bundled, false, &draft, None);
        assert_untouched(&victim, "an override's copy", i);
        let _ = std::fs::remove_dir_all(user.join("tea"));
    }
}

/// Copy to Claude Code makes `<dest>/tea` and fills it: an agent that
/// can write `~/.claude/skills` plants a link in it meanwhile.
#[test]
fn copy_to_claude_code_never_writes_through_a_planted_link() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("skills");
    source_skill(&user.join("tea"), "FROM CONTOUR");
    let dest = tmp.path().join("claude-skills");
    std::fs::create_dir_all(&dest).unwrap();
    let victim = victim(tmp.path());
    let _watch = Planter::start(plant_in_new_folders(
        vec![dest.clone()],
        |n| n == "tea",
        &["SKILL.md", user_content::EXPORT_MARK],
        victim.clone(),
    ));
    // And right at the mark's name, as soon as the folder is there.
    let (v, mark) = (
        victim.clone(),
        dest.join("tea").join(user_content::EXPORT_MARK),
    );
    let _mark = Planter::start(move || {
        let _ = std::os::unix::fs::symlink(&v, &mark);
    });
    let hash = ExportFiles::read(&user.join("tea")).unwrap().hash();
    for i in 0..10 {
        let _ = user_content::export_skill(&user, &user.join("tea"), "tea", &dest, &hash);
        assert_untouched(&victim, "copy to Claude Code", i);
        let _ = std::fs::remove_dir_all(dest.join("tea"));
    }
}

/// A generated design system is built in `.installing-<random>` and its
/// files written there: a link planted in it once listed is never written
/// through.
#[test]
fn a_generated_design_system_never_writes_through_a_planted_link() {
    let tmp = tempfile::tempdir().unwrap();
    let user = tmp.path().join("design-systems");
    std::fs::create_dir_all(&user).unwrap();
    let victim = victim(tmp.path());
    let _watch = Planter::start(plant_in_new_folders(
        vec![user.clone()],
        |n| n.starts_with(".installing-"),
        &["DESIGN.md", "tokens.css", "manifest.json", "metadata.json"],
        victim.clone(),
    ));
    let req = GenerateRequest::new("Trà Đá", "Street tea stall.", None).unwrap();
    let staging = tempfile::tempdir().unwrap();
    std::fs::write(staging.path().join("DESIGN.md"), design_md()).unwrap();
    std::fs::write(staging.path().join("tokens.css"), ":root{--a:#2f6b3a}").unwrap();
    for i in 0..10 {
        let _ = design_system_gen::install(staging.path(), &user, &req);
        assert_untouched(&victim, "a generated design system", i);
    }
}

fn design_md() -> String {
    let mut s = String::from("# Trà Đá\n\n> Category: Food\n\nWarm tea shop palette.\n\n");
    for (i, sec) in design_system_gen::SECTIONS.iter().enumerate() {
        s.push_str(&format!("## {}. {sec}\n\nText.\n\n", i + 1));
    }
    s.push_str("- **Leaf** `#2f6b3a`\n- **Cream** `#f6efe2`\n");
    s
}

/// `copy_dir` makes its folder new: one already there (an agent's, with
/// a link in it) is refused, never filled.
#[test]
fn copy_dir_refuses_a_folder_already_there() {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("src/tea");
    source_skill(&src, "FROM THE SOURCE");
    let victim = victim(tmp.path());
    let to = tmp.path().join("to");
    std::fs::create_dir(&to).unwrap();
    std::os::unix::fs::symlink(&victim, to.join("SKILL.md")).unwrap();
    let copied = user_content::copy_dir(&src, &to, u64::MAX);
    assert!(matches!(copied, Err(WriteError::Exists(_))), "{copied:?}");
    assert_untouched(&victim, "copy_dir", 0);
    // A link at the folder's name: not followed either.
    let elsewhere = tmp.path().join("elsewhere");
    std::fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, tmp.path().join("linked")).unwrap();
    let copied = user_content::copy_dir(&src, &tmp.path().join("linked"), u64::MAX);
    assert!(matches!(copied, Err(WriteError::Exists(_))), "{copied:?}");
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}
