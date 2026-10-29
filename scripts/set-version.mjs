// Stamps a version into src-tauri/Cargo.toml's [package] table. The release build runs it with
// the tag's version, next to `npm pkg set version` for package.json.
//
// Usage: node scripts/set-version.mjs 2.10.0
import { readFileSync, writeFileSync } from 'node:fs';

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+(-[\w.]+)?$/.test(version ?? '')) {
  throw new Error(`expected a version like 2.10.0, got ${version}`);
}

const path = new URL('../src-tauri/Cargo.toml', import.meta.url);
const before = readFileSync(path, 'utf8');
// The first `version =` line is [package]'s: it sits right under name, above every dependency.
const after = before.replace(/^version = "[^"]*"/m, `version = "${version}"`);
if (after === before && !before.includes(`version = "${version}"`)) {
  throw new Error('no version line in src-tauri/Cargo.toml');
}
writeFileSync(path, after);
console.log(`src-tauri/Cargo.toml version = ${version}`);
