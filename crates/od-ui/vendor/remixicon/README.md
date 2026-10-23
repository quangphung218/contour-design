# Remix Icon (vendored)

The UI's icons are Remix Icon "line" glyphs (plus `stop-fill`,
`star-fill` and `checkbox-circle-fill`), copied unchanged from `remixicon@4.8.0`
(`icons/<category>/<name>.svg`), Apache License 2.0 (`LICENSE`),
Copyright Remix Design. Only the glyphs the UI uses are kept; each one is
compiled into the WASM bundle by `src/app/icons.rs` (`include_str!`).

4.8.0 is the last release under Apache-2.0: from 4.9.0 the package ships the
"Remix Icon License v1.0" instead, so do not upgrade past 4.8.0 without a
license review.

To add a glyph: `npm pack remixicon@4.8.0`, copy the SVG here, add a
`Glyph` variant in `src/app/icons.rs`.
