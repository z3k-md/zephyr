import { describe, expect, it } from 'vitest';
import { renderMarkdown } from './markdown';

const FENCE = '```';

describe('renderMarkdown', () => {
  it.each([
    ['<script>alert(1)</script>', '<p>&lt;script&gt;alert(1)&lt;/script&gt;</p>'],
    ['a & b "quoted"', '<p>a &amp; b &quot;quoted&quot;</p>'],
    ['# <b>Title</b>', '<h1>&lt;b&gt;Title&lt;/b&gt;</h1>'],
    ['`<i>`', '<p><code>&lt;i&gt;</code></p>'],
    [
      `${FENCE}\n<img src=x onerror=alert(1)>\n${FENCE}`,
      '<pre><code>&lt;img src=x onerror=alert(1)&gt;</code></pre>',
    ],
  ])('escapes %s', (source, html) => {
    expect(renderMarkdown(source)).toBe(html);
  });

  it('only links http and https', () => {
    expect(renderMarkdown('[x](javascript:alert(1))')).not.toContain('<a');
    expect(renderMarkdown('[x](https://example.com)')).toContain(
      '<a href="https://example.com">x</a>'
    );
  });

  it('keeps a quote from breaking out of a link', () => {
    const html = renderMarkdown('https://example.com/"onmouseover="alert(1)');
    expect(html).not.toContain('"onmouseover');
  });

  it.each([
    ['**bold**', '<p><strong>bold</strong></p>'],
    ['*em*', '<p><em>em</em></p>'],
    ['~~gone~~', '<p><del>gone</del></p>'],
  ])('formats %s', (source, html) => {
    expect(renderMarkdown(source)).toBe(html);
  });
});
