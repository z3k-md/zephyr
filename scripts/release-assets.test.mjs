import assert from 'node:assert/strict';
import { test } from 'node:test';
import { installerFor, platformsFor, updaterLabel, withDownloads } from './release-assets.mjs';

// The files on the v2.10.0 release, as tauri-action named them.
const V2_10_0 = [
  'latest.json',
  'Zephyr_2.10.0_aarch64.app.tar.gz',
  'Zephyr_2.10.0_aarch64.app.tar.gz.sig',
  'Zephyr_2.10.0_aarch64.dmg',
  'Zephyr_2.10.0_x64-setup.exe',
  'Zephyr_2.10.0_x64-setup.exe.sig',
];

test('every platform the release requires has a signed bundle', () => {
  // The same pairing updater-manifest.mjs does: a bundle counts once its .sig is uploaded.
  const platforms = V2_10_0.filter((name) => name.endsWith('.sig'))
    .map((sig) => sig.slice(0, -'.sig'.length))
    .filter((bundle) => V2_10_0.includes(bundle))
    .flatMap(platformsFor);
  // REQUIRED_PLATFORMS in release.yml.
  assert.deepEqual(platforms.sort(), ['darwin-aarch64', 'windows-x86_64']);
});

test('platformsFor', () => {
  assert.deepEqual(platformsFor('Zephyr_2.10.0_aarch64.app.tar.gz'), ['darwin-aarch64']);
  assert.deepEqual(platformsFor('Zephyr_2.10.0_universal.app.tar.gz'), [
    'darwin-aarch64',
    'darwin-x86_64',
  ]);
  assert.deepEqual(platformsFor('Zephyr_2.10.0_x64-setup.exe'), ['windows-x86_64']);
  assert.deepEqual(platformsFor('Zephyr_2.10.0_arm64-setup.exe'), ['windows-aarch64']);
  assert.deepEqual(platformsFor('Zephyr_2.10.0_aarch64.dmg'), []);
  assert.deepEqual(platformsFor('Zephyr_2.10.0_x64-setup.exe.sig'), []);
  assert.deepEqual(platformsFor('latest.json'), []);
});

test('every v2.10.0 file gets a label', () => {
  for (const name of V2_10_0) {
    const label = installerFor(name, '2.10.0')?.label ?? updaterLabel(name);
    assert.ok(label, `${name} has no label`);
  }
});

test('installerFor', () => {
  assert.deepEqual(installerFor('Zephyr_2.10.0_x64-setup.exe', '2.10.0'), {
    os: 'Windows',
    label: 'Zephyr 2.10.0 for Windows',
  });
  assert.deepEqual(installerFor('Zephyr_2.10.0_aarch64.dmg', '2.10.0'), {
    os: 'macOS',
    label: 'Zephyr 2.10.0 for Mac (Apple silicon)',
  });
  assert.deepEqual(installerFor('Zephyr_2.10.0_x64_en-US.msi', '2.10.0'), {
    os: 'Windows',
    label: 'Zephyr 2.10.0 for Windows (MSI)',
  });
  assert.equal(installerFor('Zephyr_2.10.0_aarch64.app.tar.gz', '2.10.0'), null);
  assert.equal(installerFor('Zephyr_2.10.0_x64-setup.exe.sig', '2.10.0'), null);
  assert.equal(installerFor('latest.json', '2.10.0'), null);
});

test('updaterLabel', () => {
  assert.match(updaterLabel('latest.json'), /manifest/);
  assert.match(updaterLabel('Zephyr_2.10.0_x64-setup.exe.sig'), /signature/);
  assert.match(updaterLabel('Zephyr_2.10.0_aarch64.app.tar.gz'), /Mac update/);
  assert.equal(updaterLabel('Zephyr_2.10.0_aarch64.dmg'), null);
});

test('withDownloads replaces its own section and keeps the notes', () => {
  const links = [{ label: 'Zephyr 2.10.0 for Windows', url: 'https://example.com/a.exe' }];
  const once = withDownloads('## Features\n\n* a thing\n', links);
  assert.match(once, /^<!-- downloads -->\n## Download\n/);
  assert.match(once, /- \*\*\[Zephyr 2\.10\.0 for Windows\]\(https:\/\/example\.com\/a\.exe\)\*\*/);
  assert.ok(once.endsWith('## Features\n\n* a thing\n'));
  assert.equal(withDownloads(once, links), once);
  assert.match(withDownloads(null, links), /<!-- \/downloads -->\n$/);
});
