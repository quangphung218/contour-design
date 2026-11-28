//! Home's template finder over the real bundled deck templates
//! (`content/design-templates`, loaded as the daemon loads them for
//! `/api/design-templates`): every card name fits in 30 characters, the
//! purpose chips hold enough templates, and briefs in English and
//! Vietnamese suggest templates of the purpose they ask for.

#![allow(clippy::unwrap_used)] // test helpers fail loudly by design

use od_content::{ContentIndex, ContentRoots};
use od_contracts::SkillSummary;
use od_ui::i18n::{Locale, set_locale};
use od_ui::intent::{Chip, templates_for};
use od_ui::template_find::{Purpose, card_name, purposes_of, role_name, suggest};
use std::path::Path;

/// The deck templates Home lists under the Slide deck chip.
fn decks() -> Vec<SkillSummary> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../content");
    let index = ContentIndex::build(&ContentRoots::bundled(&root));
    let all: Vec<SkillSummary> = index
        .design_templates
        .iter()
        .map(|e| SkillSummary {
            info: e.info.clone(),
            has_body: true,
            has_example: false,
            film: None,
        })
        .collect();
    let decks = templates_for(Chip::Deck, &all);
    assert!(
        decks.len() >= 60,
        "content/design-templates looks incomplete"
    );
    decks
}

#[test]
fn every_deck_card_name_fits_in_30_characters_in_english_and_vietnamese() {
    let decks = decks();
    let mut long = Vec::new();
    for locale in [Locale::En, Locale::Vi] {
        set_locale(locale);
        for s in &decks {
            let name = card_name(s);
            if name.chars().count() > 30 {
                long.push(format!("{} {}: {name}", locale.tag(), s.info.id));
            }
        }
    }
    set_locale(Locale::En);
    assert!(
        long.is_empty(),
        "{} names over 30:\n{}",
        long.len(),
        long.join("\n")
    );
    // The long role-play name is not lost: it reads under the short one.
    let coral = decks
        .iter()
        .find(|s| s.info.id == "html-ppt-zhangzara-coral")
        .unwrap();
    assert_eq!(card_name(coral), "Coral · Growth campaign");
    assert_eq!(
        role_name(coral).as_deref(),
        Some("Plan a Community-Growth Campaign like a Head of Growth")
    );
}

#[test]
fn each_purpose_holds_several_decks_and_most_decks_have_one() {
    let decks = decks();
    let filed: Vec<Vec<Purpose>> = decks.iter().map(purposes_of).collect();
    for p in Purpose::ALL {
        let n = filed.iter().filter(|f| f.contains(&p)).count();
        assert!(n >= 3, "{}: only {n} deck templates", p.key());
    }
    let without: Vec<&str> = decks
        .iter()
        .zip(&filed)
        .filter(|(_, f)| f.is_empty())
        .map(|(s, _)| s.info.id.as_str())
        .collect();
    assert!(
        without.len() * 100 <= decks.len() * 15,
        "{} of {} deck templates have no purpose: {without:?}",
        without.len(),
        decks.len()
    );
}

/// A brief and the purpose every suggestion must be filed under.
const BRIEFS: &[(&str, Purpose)] = &[
    ("An investor pitch deck for our seed round", Purpose::Pitch),
    ("Deck gọi vốn cho nhà đầu tư", Purpose::Pitch),
    (
        "Quarterly business review for the leadership team",
        Purpose::Report,
    ),
    ("Báo cáo KPI hàng tuần cho ban giám đốc", Purpose::Report),
    ("Onboarding training for new hires", Purpose::Training),
    (
        "Đào tạo nhân viên mới về quy trình bán lẻ",
        Purpose::Training,
    ),
    ("A launch campaign for our new product", Purpose::Sales),
    ("Chiến dịch ra mắt sản phẩm mới", Purpose::Sales),
    ("A photo essay of our trip to Kyoto", Purpose::Story),
    ("Kể chuyện chuyến du lịch Đà Lạt bằng ảnh", Purpose::Story),
];

#[test]
fn a_brief_suggests_three_templates_of_the_purpose_it_asks_for() {
    let decks = decks();
    let mut wrong = Vec::new();
    for (brief, want) in BRIEFS {
        let got = suggest(brief, &decks);
        if got.len() != 3 {
            wrong.push(format!("{brief}: {} suggestions", got.len()));
        }
        for s in &got {
            if !purposes_of(s).contains(want) {
                wrong.push(format!("{brief}: {} is not {}", s.info.id, want.key()));
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_cafe_menu_template_comes_first_for_a_cafe_menu_brief() {
    let decks = decks();
    for brief in [
        "Seasonal menu launch for our cafe",
        "Ra mắt thực đơn mùa thu của quán cà phê",
    ] {
        let got = suggest(brief, &decks);
        assert_eq!(
            got.first().map(|s| s.info.id.as_str()),
            Some("deck-menu-cards"),
            "{brief}"
        );
    }
}

#[test]
fn a_brief_on_no_purpose_suggests_nothing() {
    let decks = decks();
    for brief in ["make it darker", "ok", "Làm cho đẹp hơn"] {
        let got: Vec<String> = suggest(brief, &decks)
            .into_iter()
            .map(|s| s.info.id)
            .collect();
        assert!(got.is_empty(), "{brief}: {got:?}");
    }
}
