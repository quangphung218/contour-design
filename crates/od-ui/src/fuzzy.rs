//! Quick switcher ranking (spec 1.2): fuzzy subsequence match over file
//! paths, recently opened files first.

/// Score of `query` against `path`, `None` when the characters of `query`
/// do not all appear in order. Higher is better. Case-insensitive.
#[must_use]
pub fn score(query: &str, path: &str) -> Option<i64> {
    let q: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(0);
    }
    let p: Vec<char> = path.chars().flat_map(char::to_lowercase).collect();
    let name_start = path.rfind('/').map_or(0, |i| path[..=i].chars().count());
    let mut qi = 0;
    let mut total = 0i64;
    let mut prev: Option<usize> = None;
    for (pi, &c) in p.iter().enumerate() {
        if qi == q.len() {
            break;
        }
        if c != q[qi] {
            continue;
        }
        let mut s = 1;
        if prev.is_some_and(|x| x + 1 == pi) {
            s += 5; // consecutive run
        }
        if pi == 0 || matches!(p[pi - 1], '/' | '-' | '_' | '.' | ' ') {
            s += 4; // word start
        }
        if pi >= name_start {
            s += 2; // in the file name, not the folder
        }
        total += s;
        prev = Some(pi);
        qi += 1;
    }
    (qi == q.len()).then(|| total * 100 - i64::try_from(p.len()).unwrap_or(i64::MAX))
}

/// `paths` filtered and ordered for `query`. Files in `recent` (most recent
/// first) rank above others; with a query they get a bonus instead.
#[must_use]
pub fn rank<'a>(query: &str, paths: &'a [String], recent: &[String]) -> Vec<&'a str> {
    let recency = |p: &str| recent.iter().position(|r| r == p);
    let mut hits: Vec<(i64, &str)> = paths
        .iter()
        .filter_map(|p| {
            let s = score(query.trim(), p)?;
            let bonus = recency(p).map_or(0, |i| 10_000 - i64::try_from(i).unwrap_or(0) * 100);
            Some((s + bonus, p.as_str()))
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    hits.into_iter().map(|(_, p)| p).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn subsequence_matches_only() {
        assert!(score("idx", "index.html").is_some());
        assert!(score("xdi", "index.html").is_none());
        assert!(score("INDEX", "index.html").is_some(), "case-insensitive");
        assert_eq!(score("", "a"), Some(0));
    }

    #[test]
    fn prefers_name_starts_and_runs() {
        let files = paths(&["assets/deck-old.html", "deck.html", "docs/d-e-c-k.md"]);
        assert_eq!(rank("deck", &files, &[])[0], "deck.html");
        let files = paths(&["src/about.html", "about/src.html"]);
        assert_eq!(
            rank("about", &files, &[])[0],
            "src/about.html",
            "file name beats folder"
        );
    }

    #[test]
    fn recent_files_come_first() {
        let files = paths(&["a.html", "b.html", "c.html"]);
        let recent = paths(&["c.html", "b.html"]);
        assert_eq!(rank("", &files, &recent), ["c.html", "b.html", "a.html"]);
        assert_eq!(rank("html", &files, &recent)[0], "c.html");
        assert_eq!(rank("a", &files, &recent), ["a.html"]);
    }
}
