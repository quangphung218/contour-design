# Golden fixtures

`original-*.json` are the outputs of the original app's TypeScript loaders
(`apps/daemon/src/skills.ts` `listSkills`, `design-systems/index.ts`
`listDesignSystems`, commit `e03d5c2`) run with Node's type stripping over this
repo's `content/` folders. `body`, `dir` and `source` are dropped (absolute paths
and a single-root scan). Regenerate after changing `content/`.
