// Builds the updater's latest.json from the signed bundles on a GitHub release and uploads it.
//
// The platform builds run in parallel, so letting each one merge its entry into latest.json
// can drop a platform when two jobs finish together. This runs once, after every build.
//
// Env: GITHUB_TOKEN, GITHUB_REPOSITORY, RELEASE_ID, REQUIRED_PLATFORMS (comma separated),
// DRY_RUN=1 to print the manifest without uploading.
const API = 'https://api.github.com';
const {
  GITHUB_TOKEN,
  GITHUB_REPOSITORY,
  RELEASE_ID,
  REQUIRED_PLATFORMS = '',
  DRY_RUN,
} = process.env;

for (const [name, value] of Object.entries({ GITHUB_REPOSITORY, RELEASE_ID })) {
  if (!value) throw new Error(`${name} is required`);
}

const headers = {
  Accept: 'application/vnd.github+json',
  'X-GitHub-Api-Version': '2022-11-28',
  ...(GITHUB_TOKEN ? { Authorization: `Bearer ${GITHUB_TOKEN}` } : {}),
};

async function github(path, init = {}) {
  const response = await fetch(path.startsWith('https://') ? path : `${API}${path}`, {
    ...init,
    headers: { ...headers, ...init.headers },
  });
  if (!response.ok) {
    throw new Error(
      `${init.method ?? 'GET'} ${path} failed: ${response.status} ${await response.text()}`
    );
  }
  return response;
}

const MAC_ARCHES = { universal: ['aarch64', 'x86_64'], aarch64: ['aarch64'], x64: ['x86_64'] };
const WINDOWS_ARCHES = { x64: 'x86_64', arm64: 'aarch64', x86: 'i686' };

/** Maps an updater bundle file name to the platform keys the Tauri updater looks up. */
function platformsFor(name) {
  const mac = name.match(/_(universal|aarch64|x64)\.app\.tar\.gz$/);
  if (mac) return MAC_ARCHES[mac[1]].map((arch) => `darwin-${arch}`);
  const windows = name.match(/_(x64|arm64|x86)-setup\.exe$/);
  if (windows) return [`windows-${WINDOWS_ARCHES[windows[1]]}`];
  return [];
}

const repo = `/repos/${GITHUB_REPOSITORY}`;
const release = await (await github(`${repo}/releases/${RELEASE_ID}`)).json();
const assets = await (await github(`${repo}/releases/${RELEASE_ID}/assets?per_page=100`)).json();
const byName = new Map(assets.map((asset) => [asset.name, asset]));

// Universal bundles go first so a native build for the same architecture overwrites them.
const signatures = assets
  .filter((asset) => asset.name.endsWith('.sig'))
  .sort((a, b) => Number(b.name.includes('_universal')) - Number(a.name.includes('_universal')));

const platforms = {};
for (const sig of signatures) {
  const bundle = byName.get(sig.name.slice(0, -'.sig'.length));
  if (!bundle) continue;
  const keys = platformsFor(bundle.name);
  if (keys.length === 0) continue;

  const signature = await (
    await github(sig.url, { headers: { Accept: 'application/octet-stream' } })
  ).text();
  // Draft asset URLs use a temporary "untagged" path, so link to where the asset lives once published.
  const url = `https://github.com/${GITHUB_REPOSITORY}/releases/download/${encodeURIComponent(
    release.tag_name
  )}/${encodeURIComponent(bundle.name)}`;
  for (const key of keys) {
    platforms[key] = { signature: signature.trim(), url };
  }
}

const missing = REQUIRED_PLATFORMS.split(',')
  .map((key) => key.trim())
  .filter((key) => key && !platforms[key]);
if (missing.length > 0) {
  throw new Error(`release ${release.tag_name} has no signed bundle for ${missing.join(', ')}`);
}

const manifest = {
  version: release.tag_name.replace(/^v/, ''),
  notes: release.body ?? '',
  pub_date: new Date().toISOString(),
  platforms,
};
const body = `${JSON.stringify(manifest, null, 2)}\n`;
console.log(body);

if (DRY_RUN) process.exit(0);

const existing = byName.get('latest.json');
if (existing) {
  await github(`${repo}/releases/assets/${existing.id}`, { method: 'DELETE' });
}
const uploadUrl = release.upload_url.replace(/\{.*\}$/, '');
await github(`${uploadUrl}?name=latest.json`, {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body,
});
console.log(`uploaded latest.json to ${release.tag_name}`);
