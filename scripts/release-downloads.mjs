// Makes a GitHub release readable for people who just want to install the app: the installers
// get plain labels ("Zephyr 2.7.0 for Windows"), the files only the updater uses are labeled
// as such, and the notes open with a Downloads section linking the installers.
//
// GitHub shows an asset's label in place of its file name. The file names stay as they are,
// because latest.json points the updater at them.
//
// Env: GITHUB_TOKEN, GITHUB_REPOSITORY, RELEASE_ID, DRY_RUN=1 to print without changing
// anything. Safe to run again on the same release.
import { installerFor, updaterLabel, withDownloads } from './release-assets.mjs';

const API = 'https://api.github.com';
const { GITHUB_TOKEN, GITHUB_REPOSITORY, RELEASE_ID, DRY_RUN } = process.env;

for (const [name, value] of Object.entries({ GITHUB_REPOSITORY, RELEASE_ID })) {
  if (!value) throw new Error(`${name} is required`);
}

const headers = {
  Accept: 'application/vnd.github+json',
  'X-GitHub-Api-Version': '2022-11-28',
  ...(GITHUB_TOKEN ? { Authorization: `Bearer ${GITHUB_TOKEN}` } : {}),
};

async function github(path, init = {}) {
  const response = await fetch(`${API}${path}`, {
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

const repo = `/repos/${GITHUB_REPOSITORY}`;
const release = await (await github(`${repo}/releases/${RELEASE_ID}`)).json();
const version = release.tag_name.replace(/^v/, '');
const assets = await (await github(`${repo}/releases/${RELEASE_ID}/assets?per_page=100`)).json();

const links = [];
for (const asset of assets) {
  const installer = installerFor(asset.name, version);
  const label = installer?.label ?? updaterLabel(asset.name);
  // Built from the tag: a draft's browser_download_url names an "untagged-…" path that 404s
  // once the release is published.
  const url = `https://github.com/${GITHUB_REPOSITORY}/releases/download/${encodeURIComponent(
    release.tag_name
  )}/${encodeURIComponent(asset.name)}`;
  if (installer) links.push({ ...installer, url });
  if (!label || asset.label === label) continue;
  console.log(`${asset.name} -> ${label}`);
  if (!DRY_RUN) {
    await github(`${repo}/releases/assets/${asset.id}`, {
      method: 'PATCH',
      body: JSON.stringify({ name: asset.name, label }),
    });
  }
}

const order = { Windows: 0, macOS: 1, Linux: 2 };
links.sort((a, b) => order[a.os] - order[b.os] || a.label.localeCompare(b.label));
const body = withDownloads(release.body, links);
if (DRY_RUN) {
  console.log(body);
} else if (body !== release.body) {
  await github(`${repo}/releases/${RELEASE_ID}`, {
    method: 'PATCH',
    body: JSON.stringify({ body }),
  });
}
