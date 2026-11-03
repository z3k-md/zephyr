import { describe, expect, it } from 'vitest';
import { hostBlocked, tabsToBlock } from '../../extension/focus.js';

describe('hostBlocked', () => {
  const sites = ['youtube.com', 'x.com'];

  it('blocks a listed host and its subdomains', () => {
    expect(hostBlocked('youtube.com', sites)).toBe(true);
    expect(hostBlocked('www.youtube.com', sites)).toBe(true);
    expect(hostBlocked('m.youtube.com', sites)).toBe(true);
    expect(hostBlocked('music.youtube.com', sites)).toBe(true);
    expect(hostBlocked('x.com', sites)).toBe(true);
    expect(hostBlocked('mobile.x.com', sites)).toBe(true);
  });

  it('leaves other hosts alone', () => {
    expect(hostBlocked('notyoutube.com', sites)).toBe(false);
    expect(hostBlocked('youtube.com.evil.test', sites)).toBe(false);
    expect(hostBlocked('box.com', sites)).toBe(false);
    expect(hostBlocked('example.com', sites)).toBe(false);
    expect(hostBlocked('', sites)).toBe(false);
  });
});

describe('tabsToBlock', () => {
  const now = 1_700_000_000_000;
  const block = { sites: ['youtube.com', 'x.com'], until: now / 1000 + 60 };

  it('selects open tabs whose host is listed', () => {
    const tabs = [
      { id: 1, url: 'https://www.youtube.com/feed' },
      { id: 2, url: 'https://example.com/' },
      { id: 3, url: 'https://mobile.x.com/home' },
      { id: 4 },
      { id: 5, url: 'not a url' },
      { url: 'https://youtube.com/' },
      { id: 6, url: 'chrome-extension://abc/blocked.html?host=youtube.com' },
    ];
    expect(tabsToBlock(tabs, block, now)).toEqual([
      { id: 1, host: 'www.youtube.com' },
      { id: 3, host: 'mobile.x.com' },
    ]);
  });

  it('selects nothing once the block has ended or lists no sites', () => {
    const tabs = [{ id: 1, url: 'https://youtube.com/' }];
    expect(tabsToBlock(tabs, block, block.until * 1000)).toEqual([]);
    expect(tabsToBlock(tabs, { sites: [], until: block.until }, now)).toEqual([]);
    expect(tabsToBlock(tabs, { sites: ['youtube.com'], until: 0 }, now)).toEqual([]);
  });
});
