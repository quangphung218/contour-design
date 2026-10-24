# React runtime for component previews

Inlined by `od-preview::react_component` into the page that previews a
`.jsx`/`.tsx` file and into "Export React component as HTML". Nothing is
fetched at run time (the upstream app loads the same pieces from unpkg).

| File | Source | License |
|---|---|---|
| `react.production.min.js` | `react@18.3.1` `umd/` | MIT (`licenses/react.LICENSE`) |
| `react-dom.production.min.js` | `react-dom@18.3.1` `umd/` | MIT (`licenses/react-dom.LICENSE`) |
| `sucrase.min.js` | `sucrase@3.35.0`, bundled with its dependencies (`lines-and-columns@1.2.4`, `ts-interface-checker@0.1.13`, `@jridgewell/*`) | MIT; `ts-interface-checker` Apache-2.0 (`licenses/`) |

Rebuild `sucrase.min.js` (global `Sucrase`, only `transform` exported):

```bash
npm install sucrase@3.35.0 esbuild@0.24.0
echo "export { transform } from 'sucrase';" > entry.js
npx esbuild entry.js --bundle --minify --format=iife --global-name=Sucrase \
  --platform=browser --legal-comments=none --outfile=sucrase.min.js
```
