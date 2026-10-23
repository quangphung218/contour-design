# xterm.js (vendored)

The terminal tab renders with xterm.js, bundled here so the UI never loads
code from a CDN. Files are copied unchanged from the npm packages, minus the
`//# sourceMappingURL` line:

| File | Package | License |
|---|---|---|
| `xterm.js`, `xterm.css` | `@xterm/xterm@6.0.0` (`lib/`, `css/`) | MIT, `LICENSE` |
| `addon-fit.js` | `@xterm/addon-fit@0.11.0` (`lib/`) | MIT, `LICENSE.addon-fit` |

To update: `npm pack @xterm/xterm@<v> @xterm/addon-fit@<v>`, copy the same
files, drop the source map line, and bump the versions here and in `NOTICE`.
