// What each file Tauri uploads to a release is, by its name. updater-manifest.mjs and
// release-downloads.mjs read the names through these; release-assets.test.mjs pins them to
// real release file names, so a change in Tauri's naming fails Checks instead of a release.

const MAC_ARCHES = { universal: ['aarch64', 'x86_64'], aarch64: ['aarch64'], x64: ['x86_64'] };
const WINDOWS_ARCHES = { x64: 'x86_64', arm64: 'aarch64', x86: 'i686' };

/** Maps an updater bundle file name to the platform keys the Tauri updater looks up. */
export function platformsFor(name) {
  const mac = name.match(/_(universal|aarch64|x64)\.app\.tar\.gz$/);
  if (mac) return MAC_ARCHES[mac[1]].map((arch) => `darwin-${arch}`);
  const windows = name.match(/_(x64|arm64|x86)-setup\.exe$/);
  if (windows) return [`windows-${WINDOWS_ARCHES[windows[1]]}`];
  return [];
}

const MAC_CHIPS = {
  universal: 'Apple silicon and Intel',
  aarch64: 'Apple silicon',
  x64: 'Intel',
};
const WINDOWS_CHIPS = { x64: '', arm64: ' (Arm)', x86: ' (32-bit)' };

/** What a person downloading the file is getting, or null for files nobody opens by hand. */
export function installerFor(name, version) {
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
export function updaterLabel(name) {
  if (name === 'latest.json') return 'Update manifest (used by Zephyr to update itself)';
  if (name.endsWith('.sig')) return 'Update signature (used by Zephyr to update itself)';
  if (name.endsWith('.app.tar.gz')) return 'Mac update package (used by Zephyr to update itself)';
  return null;
}

const START = '<!-- downloads -->';
const END = '<!-- /downloads -->';

/** The notes with a Downloads section on top, replacing one an earlier run added. */
export function withDownloads(body, links) {
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
