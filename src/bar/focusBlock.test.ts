import { describe, expect, it } from 'vitest';
import { hostBlocked } from '../../extension/focus.js';

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
