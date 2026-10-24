//! A React component file (`.jsx`/`.tsx`) as a page that renders it: the
//! Studio preview of such a file and Export ▸ "Export React component as
//! HTML" (upstream `buildReactComponentSrcdoc`). React, ReactDOM and the
//! JSX/TypeScript compiler (Sucrase) are inlined from `vendor/react-runtime`,
//! so the page runs offline and fetches nothing.
//!
//! The source is rewritten before it compiles: `react` imports read the
//! global `React`, other imports are dropped, and `export`s become plain
//! declarations with the component left in `window.__NetComponent`
//! ([`prepare`]). Compile or render errors show on the page.

use regex::Regex;
use std::sync::LazyLock;

const REACT_JS: &str = include_str!("../vendor/react-runtime/react.production.min.js");
const REACT_DOM_JS: &str = include_str!("../vendor/react-runtime/react-dom.production.min.js");
const SUCRASE_JS: &str = include_str!("../vendor/react-runtime/sucrase.min.js");
/// React and ReactDOM keep only a short `@license` header when minified;
/// the page carries their MIT text. Sucrase's bundle starts with its own
/// and its dependencies' license texts (`build-sucrase.sh`).
const REACT_LICENSE: &str = include_str!("../vendor/react-runtime/licenses/react.LICENSE");

/// Name given to an anonymous default export.
const DEFAULT_NAME: &str = "NetComponent";

/// Whether `path` is a React component source.
#[must_use]
pub fn is_component(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".jsx") || lower.ends_with(".tsx")
}

/// The HTML page that compiles and renders `source`. `typescript` turns on
/// the TypeScript transform (`.tsx`).
#[must_use]
pub fn page(source: &str, title: &str, typescript: bool) -> String {
    let prepared = prepare(source);
    // A JSON string is a JS string literal; `</` must not close the script.
    let source_js = serde_json::to_string(&prepared)
        .unwrap_or_else(|_| "\"\"".to_owned())
        .replace("</", "<\\/");
    let transforms = if typescript {
        r#"["typescript","jsx"]"#
    } else {
        r#"["jsx"]"#
    };
    let title = html_escape::encode_text(if title.trim().is_empty() {
        "React component"
    } else {
        title
    });
    format!(
        r#"<!doctype html>
<!--
React and ReactDOM 18.3.1 are inlined below under this license:

{REACT_LICENSE}
-->
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<style>
:root {{ color-scheme: light; }}
* {{ box-sizing: border-box; }}
html, body, #root {{ min-height: 100%; margin: 0; }}
body {{ font-family: ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif; background: #fff; color: #111827; }}
#root {{ min-height: 100vh; }}
.net-react-error {{ margin: 16px; padding: 14px 16px; border: 1px solid #fecaca; border-radius: 8px; background: #fff1f2; color: #991b1b; font: 12px/1.5 ui-monospace, Menlo, Consolas, monospace; white-space: pre-wrap; }}
</style>
</head>
<body>
<div id="root"></div>
<script>{REACT_JS}</script>
<script>{REACT_DOM_JS}</script>
<script>{SUCRASE_JS}</script>
<script>
(function () {{
  var root = document.getElementById('root');
  function showError(err) {{
    root.textContent = '';
    var el = document.createElement('pre');
    el.className = 'net-react-error';
    el.textContent = err && (err.stack || err.message) ? (err.stack || err.message) : String(err);
    root.appendChild(el);
  }}
  try {{
    var compiled = window.Sucrase.transform({source_js}, {{ transforms: {transforms}, production: true }}).code;
    // Runs inside the sandboxed preview frame (opaque origin).
    (0, eval)('(function () {{\n' + compiled + '\n}})()');
    var Component = window.__NetComponent;
    if (!Component) {{
      throw new Error('No React component found. Export a default component or define App, Component or Preview.');
    }}
    window.ReactDOM.createRoot(root).render(window.React.createElement(Component));
  }} catch (err) {{
    showError(err);
  }}
}})();
</script>
</body>
</html>
"#
    )
}

// Specifiers never cross a `;` or a string: one import never swallows
// the lines of the ones before it.
static TYPE_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^[ \t]*import\s+type\s[^;'"]*?\sfrom\s+['"][^'"]+['"];?[ \t]*$"#)
        .expect("valid regex")
});
static REACT_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^[ \t]*import\s+([^;'"]*?)\s+from\s+['"]react['"];?[ \t]*$"#)
        .expect("valid regex")
});
static OTHER_IMPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?m)^[ \t]*import\s+(?:[^;'"]*?\s+from\s+)?['"][^'"]+['"];?[ \t]*$"#)
        .expect("valid regex")
});
static DEFAULT_FN: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"export\s+default\s+(async\s+)?(function|class)\b\s*([A-Za-z_$][\w$]*)?")
        .expect("valid regex")
});
static DEFAULT_NAME_EXPORT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"export\s+default\s+([A-Za-z_$][\w$]*)\s*;?[ \t]*(\r?\n|$)").expect("valid regex")
});
static DEFAULT_EXPR: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"export\s+default\s+").expect("valid regex"));
static NAMED_DECL: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"export\s+((?:async\s+)?function\*?|class|const|let|var|type|interface|enum|declare|abstract\s+class)\s+([A-Za-z_$][\w$]*)",
    )
    .expect("valid regex")
});
static EXPORT_LIST: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"export\s*(type\s*)?\{([^}]*)\}\s*(?:from\s+['\x22][^'\x22]+['\x22])?;?")
        .expect("valid regex")
});

/// `source` without module syntax, ending with the line that leaves the
/// component in `window.__NetComponent`: the default export, else the
/// first exported value, else `App`, `Component` or `Preview` if defined.
#[must_use]
pub fn prepare(source: &str) -> String {
    let code = TYPE_IMPORT.replace_all(source, "");
    let code = REACT_IMPORT.replace_all(&code, |c: &regex::Captures<'_>| react_bindings(&c[1]));
    let code = OTHER_IMPORT.replace_all(&code, "");

    let mut default_name: Option<String> = None;
    let code = DEFAULT_FN.replace_all(&code, |c: &regex::Captures<'_>| {
        let name = c.get(3).map_or(DEFAULT_NAME, |m| m.as_str()).to_owned();
        let out = format!("{}{} {name}", c.get(1).map_or("", |m| m.as_str()), &c[2]);
        default_name = Some(name);
        out
    });
    let code = DEFAULT_NAME_EXPORT.replace_all(&code, |c: &regex::Captures<'_>| {
        default_name = Some(c[1].to_owned());
        c[2].to_owned()
    });
    let code = DEFAULT_EXPR.replace_all(&code, |_: &regex::Captures<'_>| {
        default_name = Some(DEFAULT_NAME.to_owned());
        format!("const {DEFAULT_NAME} = ")
    });
    let mut first_named: Option<String> = None;
    let code = NAMED_DECL.replace_all(&code, |c: &regex::Captures<'_>| {
        let kind = &c[1];
        let is_value = !matches!(kind, "type" | "interface" | "declare");
        if is_value && first_named.is_none() {
            first_named = Some(c[2].to_owned());
        }
        format!("{kind} {}", &c[2])
    });
    let code = EXPORT_LIST.replace_all(&code, |c: &regex::Captures<'_>| {
        if c.get(1).is_none() {
            for spec in c[2].split(',').map(str::trim) {
                match spec.split_once(" as ") {
                    Some((local, alias)) if alias.trim() == "default" => {
                        default_name = Some(local.trim().to_owned());
                    }
                    Some((local, _)) if first_named.is_none() => {
                        first_named = Some(local.trim().to_owned());
                    }
                    None if !spec.is_empty() && first_named.is_none() => {
                        first_named = Some(spec.to_owned());
                    }
                    _ => {}
                }
            }
        }
        String::new()
    });

    let fallback = match default_name.or(first_named) {
        Some(name) => name,
        None => ["App", "Component", "Preview"]
            .iter()
            .map(|n| format!("(typeof {n} !== 'undefined' ? {n} : null)"))
            .collect::<Vec<_>>()
            .join(" || "),
    };
    format!("{code}\nwindow.__NetComponent = window.__NetComponent || ({fallback});")
}

/// `const` bindings standing in for `import <specifier> from 'react'`.
fn react_bindings(specifier: &str) -> String {
    let spec = specifier.trim();
    if let Some(ns) = spec.strip_prefix('*') {
        let name = ns.trim().trim_start_matches("as").trim();
        return format!("const {name} = window.React;");
    }
    let (default, named) = match (spec.find('{'), spec.rfind('}')) {
        (Some(open), Some(close)) if open < close => (
            spec[..open].trim().trim_end_matches(',').trim(),
            Some(&spec[open + 1..close]),
        ),
        _ => (spec, None),
    };
    let mut out = Vec::new();
    // `React` itself is already the global the compiled JSX calls.
    if !default.is_empty() && default != "React" && !default.starts_with("type ") {
        out.push(format!("const {default} = window.React;"));
    }
    if let Some(named) = named {
        let names: Vec<String> = named
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty() && !p.starts_with("type "))
            .map(|p| p.replace(" as ", ": "))
            .collect();
        if !names.is_empty() {
            out.push(format!("const {{ {} }} = window.React;", names.join(", ")));
        }
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_read_the_global_react_and_others_go() {
        let src = "import React, { useState, type FC, useMemo as memo } from 'react';\n\
                   import type { Props } from './types';\n\
                   import './card.css';\n\
                   import {\n  Button,\n} from '@/ui';\n\
                   export default function Card() { const [n] = useState(1); return <b>{n}</b>; }\n";
        let out = prepare(src);
        assert!(
            out.contains("const { useState, useMemo: memo } = window.React;"),
            "{out}"
        );
        assert!(!out.contains("import"), "{out}");
        assert!(!out.contains("export"), "{out}");
        assert!(out.contains("function Card()"), "{out}");
        assert!(out.ends_with("window.__NetComponent = window.__NetComponent || (Card);"));
        assert_eq!(
            prepare("import * as R from \"react\";\nconst App = () => null;"),
            "const R = window.React;\nconst App = () => null;\nwindow.__NetComponent = window.__NetComponent || ((typeof App !== 'undefined' ? App : null) || (typeof Component !== 'undefined' ? Component : null) || (typeof Preview !== 'undefined' ? Preview : null));"
        );
    }

    #[test]
    fn an_import_before_the_react_one_stays_on_its_own_line() {
        let src = "import './x.css';\nimport clsx from 'clsx';\nimport type { P } from './p';\n\
                   import React, { useState } from 'react';\n\
                   export default function A() { const [n] = useState(clsx('a')); return <i>{n}</i>; }\n";
        let out = prepare(src);
        assert_eq!(
            out.lines().find(|l| !l.trim().is_empty()),
            Some("const { useState } = window.React;"),
            "{out}"
        );
        assert!(!out.contains("import") && !out.contains("from '"), "{out}");
        assert!(out.contains("function A()"), "{out}");
    }

    #[test]
    fn the_component_is_the_default_then_the_first_export() {
        let tail = |s: &str| {
            prepare(s)
                .rsplit('\n')
                .next()
                .unwrap_or_default()
                .to_owned()
        };
        assert!(tail("export default () => <i/>;").ends_with("(NetComponent);"));
        assert!(prepare("export default () => <i/>;").starts_with("const NetComponent = () =>"));
        assert!(tail("export default function () { return null }").ends_with("(NetComponent);"));
        assert!(tail("const Hero = () => null;\nexport default Hero;\n").ends_with("(Hero);"));
        assert!(
            tail("export interface P { a: 1 }\nexport const Card = () => null;")
                .ends_with("(Card);")
        );
        assert!(tail("const A = 1; const B = 2;\nexport { A, B as default };").ends_with("(B);"));
        assert!(tail("export async function Load() {}").ends_with("(Load);"));
    }

    #[test]
    fn the_page_is_self_contained() {
        let html = page(
            "export default () => <p>{\"</script>\"}</p>;",
            "A <b>",
            true,
        );
        assert!(html.contains("<title>A &lt;b&gt;</title>"));
        assert!(html.contains(r#"transforms: ["typescript","jsx"]"#));
        assert!(
            !html.contains("<script src") && !html.contains("<link"),
            "fetches nothing"
        );
        // Only the four script tags close: the source cannot end one.
        assert_eq!(html.matches("</script>").count(), 4);
        for js in [REACT_JS, REACT_DOM_JS, SUCRASE_JS] {
            assert!(!js.contains("</script"));
        }
        assert!(page("x", "", false).contains(r#"transforms: ["jsx"]"#));
        // Every inlined library comes with its license text.
        for notice in [
            "Copyright (c) Facebook, Inc. and its affiliates.",
            "Permission is hereby granted, free of charge",
            "Copyright (c) 2012-2018 various contributors (see AUTHORS)",
            "Copyright (c) 2015 Brian Donovan",
            "---- ts-interface-checker ----",
            "Apache License",
            "Justin Ridgewell",
        ] {
            assert!(html.contains(notice), "missing notice: {notice}");
        }
        assert!(!REACT_LICENSE.contains("--"), "fits an HTML comment");
    }
}
