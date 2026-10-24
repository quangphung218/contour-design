# React runtime for component previews

Inlined by `od-preview::react_component` into the page that previews a
`.jsx`/`.tsx` file and into "Export React component as HTML". Nothing is
fetched at run time (the upstream app loads the same pieces from unpkg).

| File | Source | License |
|---|---|---|
| `react.production.min.js` | `react@18.3.1` `umd/` | MIT (`licenses/react.LICENSE`) |
| `react-dom.production.min.js` | `react-dom@18.3.1` `umd/` | MIT (`licenses/react-dom.LICENSE`) |
| `sucrase.min.js` | `sucrase@3.35.0`, bundled with its dependencies (`lines-and-columns@1.2.4`, `ts-interface-checker@0.1.13`, `@jridgewell/gen-mapping@0.3.13`, `@jridgewell/resolve-uri@3.1.2`, `@jridgewell/sourcemap-codec@1.6.0`, `@jridgewell/trace-mapping@0.3.31`) | MIT; `ts-interface-checker` Apache-2.0 (`licenses/`) |

Rebuild `sucrase.min.js` (global `Sucrase`, only `transform` exported). The
script puts the license text of Sucrase and of every package bundled with it
in a leading `/*! … */` comment, so an exported page carries the notices:

```bash
mkdir /tmp/sucrase-build && cd /tmp/sucrase-build
npm install sucrase@3.35.0 esbuild@0.24.0
bash <repo>/crates/od-preview/vendor/react-runtime/build-sucrase.sh <repo>/crates/od-preview/vendor/react-runtime
```

React and ReactDOM keep their `@license` header; the page adds their MIT text
in an HTML comment (`react_component.rs`).
