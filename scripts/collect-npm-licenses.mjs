// Build-time only. Preserve installed dependency notices alongside embedded UI.
import { mkdir, readFile, readdir, copyFile } from 'node:fs/promises';
import path from 'node:path';

const [source, output] = process.argv.slice(2);
if (!source || !output) throw new Error('Usage: collect-npm-licenses.mjs NODE_MODULES OUTPUT');
const inventory = [];
async function collect(directory) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (!entry.isDirectory() || entry.name.startsWith('.')) continue;
    const location = path.join(directory, entry.name);
    if (entry.name.startsWith('@')) {
      await collect(location);
      continue;
    }
    let pkg;
    try { pkg = JSON.parse(await readFile(path.join(location, 'package.json'), 'utf8')); }
    catch (error) { if (error.code === 'ENOENT') continue; throw error; }
    const id = `${pkg.name}@${pkg.version}`.replace(/[^a-zA-Z0-9._@-]/g, '_');
    const target = path.join(output, id);
    await mkdir(target, { recursive: true });
    const notices = [];
    for (const file of await readdir(location, { withFileTypes: true })) {
      if (file.isFile() && /^(licen[sc]e|copying|notice|ofl)([._-]|$)/i.test(file.name)) {
        await copyFile(path.join(location, file.name), path.join(target, file.name));
        notices.push(file.name);
      }
    }
    inventory.push({ name: pkg.name, version: pkg.version, license: pkg.license ?? null, notices });
    try { await collect(path.join(location, 'node_modules')); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
}
await mkdir(output, { recursive: true });
await collect(source);
const { writeFile } = await import('node:fs/promises');
await writeFile(path.join(output, 'inventory.json'), JSON.stringify(inventory, null, 2) + '\n');
console.log(`Preserved license inventory and available notices for ${inventory.length} npm packages.`);
