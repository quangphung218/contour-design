// Lets Node load the original daemon's TypeScript without its build or
// node_modules: `./x.js` imports resolve to `./x.ts`, missing packages to an
// empty module, and `db.ts` (SQLite) to the one function `skills.ts` calls.
import { registerHooks } from 'node:module';
import { existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

registerHooks({
  resolve(spec, ctx, next) {
    if (spec.startsWith('.') && spec.endsWith('.js')) {
      const js = fileURLToPath(new URL(spec, ctx.parentURL));
      if (!existsSync(js) && existsSync(js.replace(/\.js$/, '.ts'))) return next(spec.replace(/\.js$/, '.ts'), ctx);
    }
    try {
      return next(spec, ctx);
    } catch {
      return { url: 'data:text/javascript,export default {};', shortCircuit: true };
    }
  },
  load(url, ctx, next) {
    if (url.endsWith('/apps/daemon/src/db.ts')) {
      return { format: 'module', source: 'export function getWorkspaceResourceByResourceId() { return null; }', shortCircuit: true };
    }
    return next(url, ctx);
  },
});
