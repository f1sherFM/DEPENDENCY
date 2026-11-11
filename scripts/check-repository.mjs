// Offline metadata/link checks, independent of the application runtime.
import assert from 'node:assert/strict';
import { readFile, readdir, stat } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const read = path => readFile(resolve(root, path), 'utf8');
const manifest = JSON.parse(await read('package.json'));
const lock = JSON.parse(await read('package-lock.json'));
const tauri = JSON.parse(await read('src-tauri/tauri.conf.json'));
const cargo = await read('src-tauri/Cargo.toml');
const cargoLock = await read('src-tauri/Cargo.lock');
for (const [name, version] of Object.entries({
  'package-lock.json': lock.version,
  'package-lock root': lock.packages?.['']?.version,
  'Cargo.toml': cargo.match(/^version\s*=\s*"([^"]+)"/m)?.[1],
  'Cargo.lock root': cargoLock.match(/\[\[package\]\]\s+name = "dependency"\s+version = "([^"]+)"/)?.[1],
  'tauri.conf.json': tauri.version,
})) assert.equal(version, manifest.version, `${name}: app versions differ`);
assert.equal(manifest.license, 'MIT', 'npm license must match Cargo');
assert.match(cargo, /^license\s*=\s*"MIT"/m);
assert.equal(lock.packages?.['']?.license, manifest.license, 'lock license differs');
assert.match(await read('LICENSE'), /^MIT License/);

async function markdownFiles(directory) {
  const files = [];
  for (const entry of await readdir(resolve(root, directory), { withFileTypes: true })) {
    const path = `${directory}/${entry.name}`;
    if (entry.isDirectory()) files.push(...await markdownFiles(path));
    else if (entry.name.endsWith('.md')) files.push(path);
  }
  return files;
}
const documents = ['README.md', 'CHANGELOG.md', 'CONTRIBUTING.md', ...await markdownFiles('docs'), ...await markdownFiles('.github')];
let checked = 0;
const failures = [];
for (const document of documents) {
  const source = (await read(document)).replace(/^```[^\n]*\n[\s\S]*?^```\s*$/gm, '');
  for (const match of source.matchAll(/\[[^\]]*\]\((<[^>]+>|[^\s)]+)(?:\s+"[^"]*")?\)/g)) {
    const href = match[1].replace(/^<|>$/g, '');
    if (/^(?:[a-z][a-z\d+.-]*:|#)/i.test(href)) continue;
    const path = decodeURIComponent(href.split('#')[0]);
    if (!path) continue;
    checked++;
    try { await stat(resolve(root, dirname(document), path)); }
    catch { failures.push(`${document}: missing ${href}`); }
  }
}
assert.deepEqual(failures, [], 'Broken local Markdown file links');
console.log(`Repository checks passed: version ${manifest.version}, MIT, ${documents.length} Markdown documents, ${checked} local links.`);
