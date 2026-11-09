import { defineLoader } from 'vitepress';

export interface ReleaseData {
  version: string | null;
  url: string;
  windows: string | null;
  macos: string | null;
}

const FALLBACK_URL = 'https://github.com/z3k-md/zephyr/releases/latest';

const emptyRelease: ReleaseData = {
  version: null,
  url: FALLBACK_URL,
  windows: null,
  macos: null,
};

interface GitHubAsset {
  name?: string;
  browser_download_url?: string;
}

interface GitHubRelease {
  tag_name?: string;
  html_url?: string;
  assets?: GitHubAsset[];
}

export default defineLoader({
  async load(): Promise<ReleaseData> {
    try {
      const headers: Record<string, string> = {
        Accept: 'application/vnd.github+json',
        'X-GitHub-Api-Version': '2022-11-28',
      };
      if (process.env.GITHUB_TOKEN) {
        headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;
      }

      const response = await fetch('https://api.github.com/repos/z3k-md/zephyr/releases/latest', {
        headers,
        signal: AbortSignal.timeout(10_000),
      });

      if (!response.ok) return { ...emptyRelease };

      const payload = (await response.json()) as GitHubRelease;
      const version = payload.tag_name?.replace(/^v/, '') ?? null;
      const url = payload.html_url ?? FALLBACK_URL;
      const assets = payload.assets ?? [];

      const windows =
        assets.find((asset) => asset.name?.endsWith('-setup.exe'))?.browser_download_url ?? null;
      const macos =
        assets.find((asset) => asset.name?.endsWith('.dmg'))?.browser_download_url ?? null;

      return { version, url, windows, macos };
    } catch {
      return { ...emptyRelease };
    }
  },
});
