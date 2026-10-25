// Prints the original `listSkills` output for one content folder, in the
// golden format (`body`, `dir`, `source` dropped). See ../README.md.
const [, , openDesign, folder] = process.argv;
const { listSkills } = await import(`${openDesign}/apps/daemon/src/skills.ts`);
const items = await listSkills(folder);
process.stdout.write(JSON.stringify(items.map(({ body, dir, source, ...rest }) => rest), null, 1));
