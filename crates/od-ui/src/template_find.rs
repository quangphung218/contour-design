//! Finding a template on Home without a model call: the name a card
//! shows, the purpose groups above the grid, the search box, and the three
//! templates the brief being typed suggests. Everything compares words:
//! lower case, without diacritics, singular ([`crate::mention::words_of`]).

use crate::i18n::T;
use crate::intent::{in_locale, template_name};
use crate::mention::{translated, words_of};
use od_contracts::SkillSummary;

/// What a template is for; the chips above Home's grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Purpose {
    Pitch,
    Report,
    Training,
    Sales,
    Event,
    Story,
}

impl Purpose {
    pub const ALL: [Self; 6] = [
        Self::Pitch,
        Self::Report,
        Self::Training,
        Self::Sales,
        Self::Event,
        Self::Story,
    ];

    #[must_use]
    pub fn label_key(self) -> T {
        match self {
            Self::Pitch => T::PurposePitch,
            Self::Report => T::PurposeReport,
            Self::Training => T::PurposeTraining,
            Self::Sales => T::PurposeSales,
            Self::Event => T::PurposeEvent,
            Self::Story => T::PurposeStory,
        }
    }

    /// Stable id for tests and `data-purpose`.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::Pitch => "pitch",
            Self::Report => "report",
            Self::Training => "training",
            Self::Sales => "sales",
            Self::Event => "event",
            Self::Story => "story",
        }
    }

    /// Phrases (in English) that file a template under this purpose, from
    /// its tags, category, Industry, id and English names; a brief that
    /// holds one asks for it.
    fn phrases(self) -> &'static [&'static str] {
        match self {
            Self::Pitch => &[
                "pitch",
                "fundraising",
                "investor",
                "business case",
                "proposal",
                "demo day",
                "series a",
                "seed round",
            ],
            Self::Report => &[
                "report",
                "review",
                "readout",
                "kpi",
                "analytics",
                "metric",
                "pre read",
                "consulting",
                "incident",
                "postmortem",
                "retro",
                "briefing",
                "memo",
                "standup",
                "quarterly",
            ],
            Self::Training => &[
                "training",
                "onboarding",
                "lecture",
                "coursework",
                "course",
                "workshop",
                "literacy",
                "capstone",
                "thesis",
                "lesson",
                "enablement",
                "class",
            ],
            Self::Sales => &[
                "sales",
                "marketing",
                "launch",
                "campaign",
                "gtm",
                "renewal",
                "menu",
                "announcement",
                "customer",
            ],
            Self::Event => &[
                "event",
                "exhibition",
                "conference",
                "talk",
                "festival",
                "season",
                "demo",
                "biennale",
                "keynote",
                "lecture",
                "meetup",
            ],
            Self::Story => &[
                "story",
                "portfolio",
                "photo",
                "essay",
                "manifesto",
                "life",
                "career",
                "hobby",
                "zine",
                "design craft",
                "identity",
                "promotion",
                "wedding",
                "travel",
            ],
        }
    }
}

/// Folded Vietnamese phrases a brief may use, put in English to meet the
/// templates' English metadata. Kept apart from the skill suggestions'
/// table (`mention::VI_EN`) so neither changes what the other suggests.
const VI_EN: &[(&str, &str)] = &[
    ("an toan", "safety"),
    ("bai giang", "lecture"),
    ("bai noi", "talk"),
    ("ban hang", "sales"),
    ("bang anh", "photo"),
    ("bao cao", "report"),
    ("bao ve", "defense"),
    ("ca phe", "cafe coffee"),
    ("chien dich", "campaign"),
    ("chien luoc", "strategy"),
    ("chinh sach", "policy"),
    ("dam cuoi", "wedding"),
    ("dao tao", "training"),
    ("do an", "capstone project"),
    ("doanh thu", "revenue"),
    ("du lich", "travel"),
    ("giao thong", "transit"),
    ("goi von", "pitch fundraising"),
    ("hang quy", "quarterly"),
    ("hang tuan", "weekly"),
    ("hoi nghi", "conference"),
    ("hoi nhap", "onboarding"),
    ("hoi thao", "workshop conference"),
    ("huan luyen", "training"),
    ("ke chuyen", "story"),
    ("khach hang", "customer"),
    ("ky niem", "anniversary"),
    ("luan van", "thesis"),
    ("mon moi", "menu launch"),
    ("nha dau tu", "investor"),
    ("nha hang", "restaurant"),
    ("nhan vien moi", "onboarding"),
    ("nghien cuu", "research"),
    ("ra mat", "launch"),
    ("san pham", "product"),
    ("su co", "incident"),
    ("su kien", "event"),
    ("tai chinh", "finance"),
    ("tai tro", "grant"),
    ("tang truong", "growth"),
    ("thuc don", "menu"),
    ("thuong hieu", "brand"),
    ("tiep thi", "marketing"),
    ("trien lam", "exhibition"),
    ("tuyen dung", "hiring"),
];

/// Words in a brief that say nothing of what the template is for.
const FILLER: &[&str] = &[
    "about",
    "and",
    "build",
    "can",
    "cho",
    "create",
    "cua",
    "deck",
    "design",
    "for",
    "from",
    "help",
    "into",
    "lam",
    "like",
    "make",
    "mot",
    "new",
    "nhung",
    "our",
    "page",
    "presentation",
    "slide",
    "tao",
    "that",
    "the",
    "this",
    "voi",
    "want",
    "with",
    "write",
    "you",
    "your",
];

/// The name on a template's card and in the picker: its short name in the
/// UI language (else English), else its display name.
#[must_use]
pub fn card_name(s: &SkillSummary) -> String {
    in_locale(s.info.short_name.as_ref())
        .filter(|n| !n.trim().is_empty())
        .cloned()
        .unwrap_or_else(|| template_name(s))
}

/// The long display name a short name stands in for, shown under it.
/// `None` when the card already shows the display name.
#[must_use]
pub fn role_name(s: &SkillSummary) -> Option<String> {
    let card = card_name(s);
    let full = template_name(s);
    (card != full).then_some(full)
}

/// The English text that files a template: tags, category, Industry, id
/// and English names (not the description: long and loose).
fn filing_words(s: &SkillSummary) -> Vec<String> {
    let i = &s.info;
    let mut text = i.tags.join(" ");
    for part in [
        i.category.as_deref().unwrap_or_default(),
        i.scenario.as_str(),
        i.id.as_str(),
        en(i.display_name.as_ref()).unwrap_or(&i.name),
        en(i.short_name.as_ref()).unwrap_or_default(),
    ] {
        text.push(' ');
        text.push_str(part);
    }
    words_of(&text)
}

fn en(map: Option<&std::collections::BTreeMap<String, String>>) -> Option<&str> {
    map.and_then(|m| m.get("en")).map(String::as_str)
}

fn holds(words: &[String], phrase: &[String]) -> bool {
    !phrase.is_empty() && words.windows(phrase.len()).any(|w| w == phrase)
}

fn purposes_in(words: &[String]) -> Vec<Purpose> {
    Purpose::ALL
        .into_iter()
        .filter(|p| p.phrases().iter().any(|ph| holds(words, &words_of(ph))))
        .collect()
}

/// The purposes a template is filed under (none, one or several).
#[must_use]
pub fn purposes_of(s: &SkillSummary) -> Vec<Purpose> {
    purposes_in(&filing_words(s))
}

/// Every word of `query` is in the template's names, description, tags,
/// category or Industry (any order, as a word start). An empty query
/// matches everything.
#[must_use]
pub fn matches_query(s: &SkillSummary, query: &str) -> bool {
    let wanted = words_of(query);
    if wanted.is_empty() {
        return true;
    }
    let i = &s.info;
    let mut text = filing_words(s).join(" ");
    for part in [
        card_name(s),
        template_name(s),
        in_locale(i.description_i18n.as_ref())
            .cloned()
            .unwrap_or_else(|| i.description.clone()),
    ] {
        text.push(' ');
        text.push_str(&part);
    }
    let words = words_of(&text);
    wanted
        .iter()
        .all(|w| words.iter().any(|x| x.starts_with(w.as_str())))
}

/// At most this many suggestions.
pub const SUGGEST_MAX: usize = 3;
/// A template needs this score to be suggested: one word of the brief in
/// its tags or names, or its purpose asked for.
const SUGGEST_MIN: u32 = 2;

/// Up to [`SUGGEST_MAX`] templates of `list` that the brief's words fit,
/// best first, ties in `list` order. A word of the brief scores 2 in a
/// template's tags, category, Industry or names and 1 in its description;
/// a purpose the brief names ([`Purpose`]) adds 2 to the templates filed
/// under it. Vietnamese phrases count in English too ([`VI_EN`]).
#[must_use]
pub fn suggest(brief: &str, list: &[SkillSummary]) -> Vec<SkillSummary> {
    let typed = words_of(brief);
    let mut words = translated(typed.clone(), VI_EN);
    words.extend(typed);
    let asked = purposes_in(&words);
    words.retain(|w| w.chars().count() >= 3 && !FILLER.contains(&w.as_str()));
    words.sort();
    words.dedup();
    if words.is_empty() {
        return Vec::new();
    }
    let mut scored: Vec<(u32, usize, &SkillSummary)> = list
        .iter()
        .enumerate()
        .filter_map(|(at, s)| {
            let filing = filing_words(s);
            let about =
                words_of(en(s.info.description_i18n.as_ref()).unwrap_or(&s.info.description));
            let mut score: u32 = words
                .iter()
                .map(|w| {
                    if filing.contains(w) {
                        2
                    } else if about.contains(w) {
                        1
                    } else {
                        0
                    }
                })
                .sum();
            let filed = purposes_in(&filing);
            score +=
                2 * u32::try_from(asked.iter().filter(|p| filed.contains(p)).count()).unwrap_or(0);
            (score >= SUGGEST_MIN).then_some((score, at, s))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(SUGGEST_MAX)
        .map(|(_, _, s)| s.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tpl(id: &str, en_name: &str, short: Option<&str>, tags: &[&str]) -> SkillSummary {
        let mut v = serde_json::json!({
            "id": id, "name": id, "description": format!("About {id}."), "triggers": [],
            "mode": "deck", "surface": "web", "source": "built-in", "craftRequires": [],
            "platform": null, "scenario": "", "category": null, "previewType": "html",
            "designSystemRequired": false, "defaultFor": [], "upstream": null, "featured": null,
            "fidelity": null, "speakerNotes": null, "animations": null, "examplePrompt": "",
            "aggregatesExamples": false, "critiquePolicy": null, "hasBody": true,
            "displayName": {"en": en_name}, "tags": tags,
        });
        if let Some(s) = short {
            v["shortName"] = serde_json::json!({"en": s, "vi": format!("{s} (vi)")});
        }
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn a_short_name_goes_on_the_card_and_the_long_one_under_it() {
        let s = tpl(
            "coral",
            "Plan a Community-Growth Campaign like a Head of Growth",
            Some("Coral · Growth campaign"),
            &[],
        );
        assert_eq!(card_name(&s), "Coral · Growth campaign");
        assert_eq!(
            role_name(&s).as_deref(),
            Some("Plan a Community-Growth Campaign like a Head of Growth")
        );
        let plain = tpl("menu", "Seasonal menu launch deck", None, &[]);
        assert_eq!(card_name(&plain), "Seasonal menu launch deck");
        assert_eq!(role_name(&plain), None);
    }

    #[test]
    fn purposes_come_from_tags_and_names() {
        let pitch = tpl("a", "Write a Demo Day Pitch", None, &["fundraising-pitch"]);
        assert!(purposes_of(&pitch).contains(&Purpose::Pitch));
        let training = tpl("b", "Onboard new hires", None, &["professional-training"]);
        assert_eq!(purposes_of(&training), [Purpose::Training]);
        let none = tpl("c", "Brutalist deck", None, &[]);
        assert!(purposes_of(&none).is_empty());
    }

    #[test]
    fn a_query_needs_every_word_in_any_order_and_diacritics_aside() {
        let s = tpl(
            "menu",
            "Seasonal menu launch deck",
            Some("Menu cards · Launch"),
            &["food-and-drink"],
        );
        assert!(matches_query(&s, ""));
        assert!(matches_query(&s, "launch MENU"));
        assert!(matches_query(&s, "seas"));
        assert!(matches_query(&s, "food"));
        assert!(!matches_query(&s, "menu pitch"));
    }

    #[test]
    fn the_brief_suggests_at_most_three_best_first_in_english_and_vietnamese() {
        let list = vec![
            tpl("brutal", "Brutalist deck", None, &[]),
            tpl(
                "menu",
                "Seasonal menu launch deck",
                None,
                &["food-and-drink"],
            ),
            tpl(
                "demo",
                "Write a Demo Day Pitch",
                None,
                &["fundraising-pitch"],
            ),
            tpl("book", "Investor Pitch Book", None, &["fundraising-pitch"]),
            tpl(
                "bold",
                "Series A Growth Story",
                None,
                &["series-a-pitch-deck"],
            ),
            tpl("week", "Weekly growth review", None, &["kpi"]),
        ];
        let ids = |brief: &str| -> Vec<String> {
            suggest(brief, &list)
                .into_iter()
                .map(|s| s.info.id)
                .collect()
        };
        assert_eq!(
            ids("An investor pitch for our seed round"),
            ["book", "demo", "bold"]
        );
        assert_eq!(ids("Deck gọi vốn cho nhà đầu tư"), ["book", "demo", "bold"]);
        assert_eq!(ids("Launch our new seasonal menu"), ["menu"]);
        assert_eq!(ids("Báo cáo KPI hàng tuần"), ["week"]);
        assert!(
            ids("make a deck").is_empty(),
            "filler words suggest nothing"
        );
        assert!(ids("").is_empty());
    }
}
