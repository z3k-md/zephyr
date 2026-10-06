// Turns notes saved as Markdown by earlier builds into HTML for the rich text editor. It escapes everything first and only emits the tags it
// builds itself, so text pasted into a note can never inject HTML into Zephyr's window.

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;');
}

/** Inline formatting on already-escaped text. Code spans are left untouched. */
function inline(escaped: string): string {
  return escaped
    .split(/(`[^`]+`)/)
    .map((part) => {
      if (part.startsWith('`') && part.endsWith('`') && part.length > 1) {
        return `<code>${part.slice(1, -1)}</code>`;
      }
      return part
        .replace(
          /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
          (_, label: string, url: string) => `<a href="#" data-href="${url}">${label}</a>`
        )
        .replace(
          /(^|[\s(])(https?:\/\/[^\s<)]+)/g,
          (_, lead: string, url: string) => `${lead}<a href="#" data-href="${url}">${url}</a>`
        )
        .replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>')
        .replace(/__([^_]+)__/g, '<strong>$1</strong>')
        .replace(/(^|[^*])\*([^*\s][^*]*)\*/g, '$1<em>$2</em>')
        .replace(/(^|[\s(])_([^_\s][^_]*)_/g, '$1<em>$2</em>')
        .replace(/~~([^~]+)~~/g, '<del>$1</del>');
    })
    .join('');
}

/**
 * Renders a note. Checklist boxes carry `data-line`, the source line index, so a click can
 * toggle the matching `[ ]` in the Markdown.
 */
export function renderMarkdown(source: string): string {
  const lines = source.split('\n');
  const out: string[] = [];
  let list: 'ul' | 'ol' | null = null;
  let paragraph: string[] = [];

  const flushParagraph = () => {
    if (paragraph.length) out.push(`<p>${paragraph.join('<br>')}</p>`);
    paragraph = [];
  };
  const closeList = () => {
    if (list) out.push(`</${list}>`);
    list = null;
  };

  for (let index = 0; index < lines.length; index++) {
    const line = lines[index];

    if (/^\s*```/.test(line)) {
      flushParagraph();
      closeList();
      const code: string[] = [];
      index++;
      while (index < lines.length && !/^\s*```/.test(lines[index])) {
        code.push(escapeHtml(lines[index]));
        index++;
      }
      out.push(`<pre><code>${code.join('\n')}</code></pre>`);
      continue;
    }

    const heading = /^(#{1,3})\s+(.*)$/.exec(line);
    if (heading) {
      flushParagraph();
      closeList();
      const level = heading[1].length;
      out.push(`<h${level}>${inline(escapeHtml(heading[2]))}</h${level}>`);
      continue;
    }

    if (/^\s*(-{3,}|\*{3,})\s*$/.test(line)) {
      flushParagraph();
      closeList();
      out.push('<hr>');
      continue;
    }

    const quote = /^\s*>\s?(.*)$/.exec(line);
    if (quote) {
      flushParagraph();
      closeList();
      out.push(`<blockquote>${inline(escapeHtml(quote[1]))}</blockquote>`);
      continue;
    }

    const task = /^\s*[-*]\s+\[( |x|X)\]\s+(.*)$/.exec(line);
    const bullet = /^\s*[-*]\s+(.*)$/.exec(line);
    const numbered = /^\s*\d+[.)]\s+(.*)$/.exec(line);
    if (task || bullet || numbered) {
      flushParagraph();
      const kind = numbered && !task && !bullet ? 'ol' : 'ul';
      if (list !== kind) {
        closeList();
        out.push(`<${kind}>`);
        list = kind;
      }
      if (task) {
        const done = task[1] !== ' ';
        out.push(
          `<li class="task${done ? ' done' : ''}"><input type="checkbox" data-line="${index}"${
            done ? ' checked' : ''
          }> <span>${inline(escapeHtml(task[2]))}</span></li>`
        );
      } else {
        const text = (bullet ?? numbered)![1];
        out.push(`<li>${inline(escapeHtml(text))}</li>`);
      }
      continue;
    }

    if (!line.trim()) {
      flushParagraph();
      closeList();
      continue;
    }
    closeList();
    paragraph.push(inline(escapeHtml(line)));
  }
  flushParagraph();
  closeList();
  return out.join('\n');
}
