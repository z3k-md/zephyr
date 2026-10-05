// Regenerates src-tauri/icons from the SVG masters in src-tauri/icons/source.
// icon.svg follows the macOS grid (padded squircle with shadow) and feeds the PNGs and icns.
// icon-windows.svg fills the canvas so the ico and Store tiles stay legible at 16px.
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const icons = 'src-tauri/icons';

function generate(source) {
  const out = mkdtempSync(join(tmpdir(), 'zephyr-icons-'));
  execFileSync('bunx', ['tauri', 'icon', join(icons, 'source', source), '-o', out], {
    stdio: 'inherit',
  });
  return out;
}

const mac = generate('icon.svg');
const windows = generate('icon-windows.svg');
const isWindowsAsset = (name) =>
  name === 'icon.ico' || name === 'StoreLogo.png' || name.startsWith('Square');

for (const name of readdirSync(mac)) {
  if (!/\.(png|icns|ico)$/.test(name)) continue;
  copyFileSync(join(isWindowsAsset(name) ? windows : mac, name), join(icons, name));
}
rmSync(mac, { recursive: true });
rmSync(windows, { recursive: true });
