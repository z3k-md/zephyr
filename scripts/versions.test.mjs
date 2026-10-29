import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, readFileSync, writeFileSync, copyFileSync, mkdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('..', import.meta.url));
const read = (path) => readFileSync(join(root, path), 'utf8');

test('rust-version matches the pinned toolchain', () => {
  const channel = read('rust-toolchain.toml').match(/^channel = "([^"]+)"/m)?.[1];
  const rustVersion = read('src-tauri/Cargo.toml').match(/^rust-version = "([^"]+)"/m)?.[1];
  assert.ok(channel && rustVersion);
  assert.equal(
    rustVersion,
    channel,
    'move rust-version in src-tauri/Cargo.toml with the toolchain'
  );
});

test('package.json and Cargo.toml carry the same version', () => {
  const pkg = JSON.parse(read('package.json')).version;
  const cargo = read('src-tauri/Cargo.toml').match(/^version = "([^"]+)"/m)?.[1];
  assert.equal(cargo, pkg);
});

test('set-version.mjs stamps only the package version', () => {
  const dir = mkdtempSync(join(tmpdir(), 'zephyr-version-'));
  mkdirSync(join(dir, 'scripts'));
  mkdirSync(join(dir, 'src-tauri'));
  copyFileSync(join(root, 'scripts/set-version.mjs'), join(dir, 'scripts/set-version.mjs'));
  const before = read('src-tauri/Cargo.toml').replace(/\r?\n/g, '\r\n'); // as Windows checks out
  writeFileSync(join(dir, 'src-tauri/Cargo.toml'), before);

  execFileSync(process.execPath, [join(dir, 'scripts/set-version.mjs'), '9.8.7']);
  const after = readFileSync(join(dir, 'src-tauri/Cargo.toml'), 'utf8');
  const changed = after.split('\r\n').filter((line, i) => line !== before.split('\r\n')[i]);
  assert.deepEqual(changed, ['version = "9.8.7"']);
});
