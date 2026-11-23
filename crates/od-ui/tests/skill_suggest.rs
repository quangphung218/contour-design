//! Skill suggestions on common prompts: [`suggest`] runs over the real
//! bundled catalogue (`content/skills`, loaded as the daemon loads it for
//! `/api/skills`) and each prompt has the skills that fit its topic.
//!
//! The test fails on any suggestion outside the prompt's topic, and when
//! coverage falls under the floors: 8/10 English and 7/10 Vietnamese on
//! the first twenty prompts (follow-up of #314), 75% of all forty topic
//! prompts and of each language's twenty. Prompts on no design topic
//! must get nothing.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::{ContentIndex, ContentRoots};
use od_ui::mention::{Candidate, MentionKind, SkillRow, suggest};
use std::path::Path;

/// Skills that fit a deck or a pitch.
const DECK: &[&str] = &[
    "slides",
    "pptx",
    "pptx-generator",
    "ppt-keynote",
    "frontend-slides",
    "nanobanana-ppt",
];
const LANDING: &[&str] = &[
    "frontend-design",
    "frontend-skill",
    "soft-skill",
    "taste-skill",
];
const CHART: &[&str] = &["d3-visualization", "data-report"];
const DASHBOARD: &[&str] = &["data-report", "frontend-design", "d3-visualization"];
const REDESIGN: &[&str] = &["redesign-existing-projects", "taste-skill", "soft-skill"];
const VIDEO: &[&str] = &[
    "video-hyperframes",
    "motion-design",
    "remotion",
    "sora",
    "vfx-text-cursor",
];
const RELEASE: &[&str] = &["release-notes-one-pager"];
const ONBOARDING: &[&str] = &["imagegen-frontend-mobile", "frontend-design"];
const ANIMATION: &[&str] = &[
    "animate",
    "ui-animation",
    "emilkowalski-motion",
    "gsap-core",
    "motion-lab",
];
const POSTER: &[&str] = &["poster-hero", "canvas-design"];
const RESUME: &[&str] = &["resume-modern"];
const BRAND: &[&str] = &["brandkit", "brand-guidelines"];
const EMAIL: &[&str] = &["copywriting"];
const SOCIAL: &[&str] = &[
    "social-x-post-card",
    "card-twitter",
    "card-xiaohongshu",
    "social-reddit-card",
];
const REPORT: &[&str] = &["data-report", "d3-visualization"];
const WIREFRAME: &[&str] = &[
    "frontend-design",
    "imagegen-frontend-mobile",
    "imagegen-frontend-web",
    "hand-drawn-diagrams",
];
const ICON: &[&str] = &["imagegen", "imagen"];
const MOCKUP_3D: &[&str] = &["mockup-device-3d", "imagegen", "threejs", "fal-3d"];
const INFOGRAPHIC: &[&str] = &["data-report", "d3-visualization", "canvas-design"];

/// `(prompt, the skills that fit it)`: the first ten English and ten
/// Vietnamese are the table of #314, then ten new topics in each.
const EN_OLD: [(&str, &[&str]); 10] = [
    ("make an interactive chart with d3 for sales", CHART),
    ("Create a pitch deck for our seed round", DECK),
    (
        "Design a landing page for a coffee subscription startup",
        LANDING,
    ),
    (
        "Build a dashboard showing monthly revenue and churn",
        DASHBOARD,
    ),
    (
        "Redesign our existing project homepage to look premium",
        REDESIGN,
    ),
    (
        "Make a short product launch video with animated text",
        VIDEO,
    ),
    ("Write release notes for version 2.3", RELEASE),
    (
        "Generate a mobile app onboarding flow for a fitness tracker",
        ONBOARDING,
    ),
    (
        "Add smooth entrance animations to the hero section",
        ANIMATION,
    ),
    ("Turn this blog post into a slide presentation", DECK),
];
const VI_OLD: [(&str, &[&str]); 10] = [
    ("Làm biểu đồ tương tác bằng d3 cho doanh số", CHART),
    ("Tạo pitch deck cho vòng gọi vốn seed", DECK),
    ("Thiết kế landing page cho startup bán cà phê", LANDING),
    (
        "Làm dashboard doanh thu theo tháng và tỉ lệ rời bỏ",
        DASHBOARD,
    ),
    ("Thiết kế lại trang chủ cho trông cao cấp hơn", REDESIGN),
    ("Làm video giới thiệu sản phẩm có chữ chuyển động", VIDEO),
    ("Viết release notes cho phiên bản 2.3", RELEASE),
    (
        "Thiết kế màn hình onboarding cho app tập thể dục",
        ONBOARDING,
    ),
    (
        "Thêm hiệu ứng chuyển động khi xuất hiện cho phần hero",
        ANIMATION,
    ),
    ("Biến bài blog này thành bộ slide thuyết trình", DECK),
];
const NEW_EN: [(&str, &[&str]); 10] = [
    ("Design a poster for our jazz night concert", POSTER),
    ("Make a one-page resume for a frontend engineer", RESUME),
    ("Create a logo and brand identity for a bakery", BRAND),
    ("Write a marketing email announcing our spring sale", EMAIL),
    ("Create a social media post about our new feature", SOCIAL),
    ("Turn this CSV into a data report with charts", REPORT),
    (
        "Sketch a wireframe for the checkout of a booking app",
        WIREFRAME,
    ),
    ("Design an icon set for a weather app", ICON),
    (
        "Make a 3D mockup of our app on an iPhone and a MacBook",
        MOCKUP_3D,
    ),
    (
        "Create an infographic about coffee consumption",
        INFOGRAPHIC,
    ),
];
const NEW_VI: [(&str, &[&str]); 10] = [
    ("Thiết kế áp phích cho đêm nhạc jazz", POSTER),
    ("Tạo sơ yếu lý lịch một trang cho kỹ sư frontend", RESUME),
    (
        "Thiết kế logo và bộ nhận diện thương hiệu cho tiệm bánh",
        BRAND,
    ),
    (
        "Viết email marketing thông báo đợt giảm giá mùa xuân",
        EMAIL,
    ),
    ("Làm bài đăng mạng xã hội giới thiệu tính năng mới", SOCIAL),
    ("Biến file CSV này thành báo cáo số liệu có biểu đồ", REPORT),
    (
        "Vẽ bản phác thảo giao diện cho màn thanh toán của app đặt phòng",
        WIREFRAME,
    ),
    ("Thiết kế bộ biểu tượng cho app thời tiết", ICON),
    (
        "Dựng mô hình 3D trưng bày app trên iPhone và MacBook",
        MOCKUP_3D,
    ),
    ("Làm đồ hoạ thông tin về lượng tiêu thụ cà phê", INFOGRAPHIC),
];

/// Everyday chat and work on no design topic: any suggestion is off
/// topic. Several read, without diacritics, like a phrase of `VI_EN` that
/// was left out ("anh đồng ý" / "ảnh động") or hold a word a trigger
/// uses in another sense ("resume the run").
const NONE: [&str; 26] = [
    "Anh đồng ý, làm tiếp phần còn lại đi",
    "ok em cứ làm tiếp đi",
    "Đồng hồ chạy sai giờ rồi",
    "Thêm ảnh đồng hồ vào trang sản phẩm",
    "Chụp ảnh đồng phục cho nhân viên",
    "Anh Đông nhờ em xem lại cái này",
    "Bài đang viết dở, em viết tiếp nhé",
    "Mở trang dịch thuật của app",
    "Mở bảng điều khiển cài đặt của máy",
    "Người ta thường hiểu sai chỗ này",
    "Hôm nay cv nhiều quá, mai làm tiếp",
    "sửa lỗi build trên Windows giúp anh",
    "merge PR này khi CI xanh",
    "xong chưa em ơi",
    "Sửa lỗi đăng nhập trên server của em",
    "Giải thích đoạn code này làm gì",
    "Resume the run from where it stopped",
    "Resume where we left off on the checkout page",
    "Make the upload resume after a network error",
    "Fix the failing unit test in the parser",
    "Rename the variable in this function please",
    "Merge this PR when CI is green",
    "Is it done yet?",
    "ok go ahead and continue",
    "Thanks, that works now",
    "Why is the build failing on Windows?",
];

/// The skill rows the composer builds from `/api/skills`.
fn catalogue() -> Vec<SkillRow> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let index = ContentIndex::build(&ContentRoots::bundled(&root));
    assert!(index.skills.len() >= 150, "content/skills looks incomplete");
    index
        .skills
        .iter()
        .map(|s| SkillRow {
            triggers: s
                .info
                .triggers
                .iter()
                .filter_map(|t| t.as_str().map(str::to_owned))
                .collect(),
            candidate: Candidate {
                kind: MentionKind::Skill,
                label: s.info.id.clone(),
                detail: String::new(),
                id: s.info.id.clone(),
                inputs: None,
            },
        })
        .collect()
}

/// How many of `cases` get a suggestion; every suggestion outside a
/// case's topic is pushed onto `wrong`, every line onto `table`.
fn run(
    skills: &[SkillRow],
    cases: &[(&str, &[&str])],
    wrong: &mut Vec<String>,
    table: &mut Vec<String>,
) -> usize {
    let mut hits = 0;
    for (prompt, fits) in cases {
        let got: Vec<String> = suggest(prompt, skills, &[])
            .into_iter()
            .map(|c| c.id)
            .collect();
        for id in &got {
            if !fits.contains(&id.as_str()) {
                wrong.push(format!("{prompt:?} → {id} (fits: {fits:?})"));
            }
        }
        if !got.is_empty() {
            hits += 1;
        }
        table.push(format!(
            "| {prompt} | {} |",
            if got.is_empty() {
                "—".to_owned()
            } else {
                got.join(", ")
            }
        ));
    }
    hits
}

#[test]
fn common_prompts_get_a_skill_of_their_topic() {
    let skills = catalogue();
    let mut wrong = Vec::new();
    let mut table = Vec::new();
    let en = run(&skills, &EN_OLD, &mut wrong, &mut table);
    let vi = run(&skills, &VI_OLD, &mut wrong, &mut table);
    let new_en = run(&skills, &NEW_EN, &mut wrong, &mut table);
    let new_vi = run(&skills, &NEW_VI, &mut wrong, &mut table);
    let none: Vec<(&str, &[&str])> = NONE.iter().map(|p| (*p, &[][..])).collect();
    run(&skills, &none, &mut wrong, &mut table);
    let total = en + vi + new_en + new_vi;
    let all = 40;
    println!("{}", table.join("\n"));
    println!("EN {en}/10 + {new_en}/10, VI {vi}/10 + {new_vi}/10, total {total}/{all}");
    assert!(
        wrong.is_empty(),
        "off-topic suggestions:\n{}",
        wrong.join("\n")
    );
    assert!(en >= 8, "English: {en}/10 prompts get a suggestion, want 8");
    assert!(
        vi >= 7,
        "Vietnamese: {vi}/10 prompts get a suggestion, want 7"
    );
    assert!(
        total * 4 >= all * 3,
        "all prompts: {total}/{all} get a suggestion, want 75%"
    );
    // Each language on its own, so the other cannot carry it.
    for (lang, n) in [("English", en + new_en), ("Vietnamese", vi + new_vi)] {
        assert!(n >= 15, "{lang}: {n}/20 prompts get a suggestion, want 75%");
    }
}
