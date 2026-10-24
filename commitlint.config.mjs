// Conventional Commits, Angular flavour: `type(scope): subject`.
// Used by the CI "commit messages" job and `make commitlint`; CONTRIBUTING.md
// explains the rules. Self-contained (no `extends`) so `npx @commitlint/cli`
// needs no other package.
import { readdirSync } from 'node:fs';

const crates = readdirSync(new URL('./crates/', import.meta.url), { withFileTypes: true })
  .filter((d) => d.isDirectory())
  .map((d) => d.name);

export default {
  rules: {
    'type-enum': [2, 'always', ['build', 'chore', 'ci', 'docs', 'feat', 'fix', 'perf', 'refactor', 'revert', 'style', 'test']],
    'type-case': [2, 'always', 'lower-case'],
    'type-empty': [2, 'never'],
    'scope-enum': [2, 'always', [...crates, 'desktop', 'brand', 'e2e', 'ci', 'docs', 'build', 'release', 'deps']],
    'scope-case': [2, 'always', 'lower-case'],
    'scope-empty': [2, 'never'],
    'subject-empty': [2, 'never'],
    'subject-case': [2, 'never', ['sentence-case', 'start-case', 'pascal-case', 'upper-case']],
    'subject-full-stop': [2, 'never', '.'],
    'header-max-length': [2, 'always', 72],
    'body-leading-blank': [2, 'always'],
    'body-max-line-length': [2, 'always', 100],
    'footer-leading-blank': [2, 'always'],
  },
};
