# Golden fixtures

`original-*.json` are the outputs of the original app's TypeScript loaders
(`apps/daemon/src/skills.ts` `listSkills`, `design-systems/index.ts`
`listDesignSystems`, commit `e03d5c2`) run with Node's type stripping over this
repo's `content/` folders. `body`, `dir` and `source` are dropped (absolute paths
and a single-root scan). Regenerate after changing `content/`:
`regen/run.sh [path to open-design at e03d5c2]` rewrites the skills and
design-templates files (no build or `npm install` of the original needed).

One Contour extension is not in the original loader: `od.examples.<key>`
(`en_name`, `en_description`) gives a derived example its own UI name and
description, so `bundled.rs` compares derived entries without `displayName`
and `descriptionI18n`.
