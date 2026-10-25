// Adds up test-results/qa/*.json written by lib.mjs and prints the QA
// summary. Exit 1 when a test failed for an unknown reason or a suite did
// not report at all.
import { readdirSync, readFileSync, existsSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..', 'test-results', 'qa');
const expected = readdirSync(here).filter((f) => f.endsWith('.qa.mjs')).map((f) => f.replace('.qa.mjs', ''));
const filter = process.argv[2];
const count = { pass: 0, known: 0, fail: 0, skip: 0, 'fixed?': 0 };
const lines = { known: [], fail: [], 'fixed?': [], missing: [] };
for (const suite of expected) {
  if (filter && !suite.includes(filter)) continue;
  const file = join(out, `${suite}.json`);
  if (!existsSync(file)) {
    lines.missing.push(suite);
    continue;
  }
  for (const r of JSON.parse(readFileSync(file, 'utf8')).results) {
    count[r.status]++;
    const label = `${suite}: ${r.id} ${r.name}`;
    if (r.status === 'known') lines.known.push(`${r.knownBug}  ${label}`);
    if (r.status === 'fail') lines.fail.push(`${label}\n      ${(r.error || '').split('\n')[0]}`);
    if (r.status === 'fixed?') lines['fixed?'].push(`${r.knownBug}  ${label}`);
  }
}
const total = Object.values(count).reduce((a, b) => a + b, 0);
console.log('\n==== QA summary');
console.log(`tests: ${total}  pass: ${count.pass}  known bug: ${count.known}  unexpected fail: ${count.fail}` +
  `  skipped: ${count.skip}  known bug now passing: ${count['fixed?']}`);
if (lines.known.length) console.log('\nfailing because of a known bug:\n  ' + lines.known.sort().join('\n  '));
if (lines['fixed?'].length) console.log('\nmarked as a known bug but passing (close the bug?):\n  ' + lines['fixed?'].join('\n  '));
if (lines.fail.length) console.log('\nunexpected failures:\n  ' + lines.fail.join('\n  '));
if (lines.missing.length) console.log('\nsuites that did not report (crashed?): ' + lines.missing.join(', '));
process.exit(lines.fail.length || lines.missing.length ? 1 : 0);
