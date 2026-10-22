// Makes a GitHub release readable for people who just want to install the app: the installers
// get plain labels ("Zephyr 2.7.0 for Windows"), the files only the updater uses are labeled
// as such, and the notes open with a Downloads section linking the installers.
//
// GitHub shows an asset's label in place of its file name. The file names stay as they are,
// because latest.json points the updater at them.
//
// Env: GITHUB_TOKEN, GITHUB_REPOSITORY, RELEASE_ID, DRY_RUN=1 to print without changing
// anything. Safe to run again on the same release.
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

const MAC_CHIPS = {
  universal: 'Apple silicon and Intel',
  aarch64: 'Apple silicon',
  x64: 'Intel',
};
const WINDOWS_CHIPS = { x64: '', arm64: ' (Arm)', x86: ' (32-bit)' };

/** What a person downloading the file is getting, or null for files nobody opens by hand. */
function installerFor(name, version) {
  const windows = name.match(/_(x64|arm64|x86)-setup\.exe$/);
  if (windows)
    return { os: 'Windows', label: `Zephyr ${version} for Windows${WINDOWS_CHIPS[windows[1]]}` };
  const windowsMsi = name.match(/_(x64|arm64|x86)_[\w-]+\.msi$/);
  if (windowsMsi) {
    return {
      os: 'Windows',
      label: `Zephyr ${version} for Windows${WINDOWS_CHIPS[windowsMsi[1]]} (MSI)`,
    };
  }
  const mac = name.match(/_(universal|aarch64|x64)\.dmg$/);
  if (mac) return { os: 'macOS', label: `Zephyr ${version} for Mac (${MAC_CHIPS[mac[1]]})` };
  if (/\.AppImage$/.test(name))
    return { os: 'Linux', label: `Zephyr ${version} for Linux (AppImage)` };
  if (/\.deb$/.test(name)) return { os: 'Linux', label: `Zephyr ${version} for Linux (.deb)` };
  if (/\.rpm$/.test(name)) return { os: 'Linux', label: `Zephyr ${version} for Linux (.rpm)` };
  return null;
}

/** The label for a file only the updater reads. */
function updaterLabel(name) {
  if (name === 'latest.json') return 'Update manifest (used by Zephyr to update itself)';
  if (name.endsWith('.sig')) return 'Update signature (used by Zephyr to update itself)';
  if (name.endsWith('.app.tar.gz')) return 'Mac update package (used by Zephyr to update itself)';
  return null;
}

const START = '<!-- downloads -->';
const END = '<!-- /downloads -->';

/** The notes with a Downloads section on top, replacing one an earlier run added. */
function withDownloads(body, links) {
  const rest = (body ?? '').replace(new RegExp(`${START}[\\s\\S]*?${END}\\n*`), '');
  const lines = links.map((link) => `- **[${link.label}](${link.url})**`);
  const section = [
    START,
    '## Download',
    '',
    ...lines,
    '',
    'Linux builds are coming later. Already installed? Zephyr updates itself.',
    END,
    '',
  ].join('\n');
  return `${section}\n${rest}`.trimEnd() + '\n';
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
