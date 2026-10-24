import { describe, expect, it } from 'vitest';
import { type Block, closedUpTo, IncrementalMarkdown, MAX_RENDER_CHARS, parseInline, parseMarkdown } from './markdown';
import type { AssistantCitation } from './wire';

const cites: AssistantCitation[] = [
  { text: 'event 12', target: 'record:12' },
  { text: 'level >= error', target: 'query:level >= error' },
  { text: 'the docs', target: 'https://learn.example.com/t', title: 'Docs' },
  { text: 'plain', target: 'http://insecure.example.com/' },
];

describe('parseInline', () => {
  it('keeps markup in model text as plain text', () => {
    const evil = '<img src=x onerror=alert(1)> <script>alert(2)</script> [click](javascript:alert(3)) javascript:alert(4)';
    expect(parseInline(evil, cites)).toEqual([{ kind: 'text', text: evil }]);
  });

  it('formats code, bold and italics', () => {
    expect(parseInline('a `x = 1` **b** __c__ *d* _e_ f')).toEqual([
      { kind: 'text', text: 'a ' },
      { kind: 'code', text: 'x = 1' },
      { kind: 'text', text: ' ' },
      { kind: 'strong', text: 'b' },
      { kind: 'text', text: ' ' },
      { kind: 'strong', text: 'c' },
      { kind: 'text', text: ' ' },
      { kind: 'em', text: 'd' },
      { kind: 'text', text: ' ' },
      { kind: 'em', text: 'e' },
      { kind: 'text', text: ' f' },
    ]);
  });

  it('links only cited phrases, backticked citations and cited https pages', () => {
    const out = parseInline('See event 12, event 99, `level >= error` or `other` at https://learn.example.com/t. Also http://insecure.example.com/ and https://uncited.example.com and the docs.', cites);
    expect(out.filter((i) => i.kind !== 'text')).toEqual([
      { kind: 'cite', text: 'event 12', target: 'record:12', title: 'event 12' },
      { kind: 'cite', text: 'level >= error', target: 'query:level >= error', title: 'level >= error' },
      { kind: 'code', text: 'other' },
      { kind: 'cite', text: 'https://learn.example.com/t', target: 'https://learn.example.com/t', title: 'Docs' },
      { kind: 'cite', text: 'the docs', target: 'https://learn.example.com/t', title: 'Docs' },
    ]);
    expect(out.map((i) => i.text).join('')).toContain('event 99');
  });

  it('does not link a phrase inside a word, inside a cited URL, or anything without citations', () => {
    expect(parseInline('prevent 12x event 12abc', cites)).toEqual([{ kind: 'text', text: 'prevent 12x event 12abc' }]);
    const c: AssistantCitation[] = [{ text: 'x', target: 'a' }, { text: 'T', target: 'https://x.example/x', title: 'T' }];
    expect(parseInline('https://x.example/x', c)).toEqual([{ kind: 'cite', text: 'https://x.example/x', target: 'https://x.example/x', title: 'T' }]);
    expect(parseInline('event 5 and #6')).toEqual([{ kind: 'text', text: 'event 5 and #6' }]);
    // A phrase with regular-expression characters is matched as written.
    expect(parseInline('a (b+c) d', [{ text: '(b+c)', target: 't' }]).map((i) => i.kind)).toEqual(['text', 'cite', 'text']);
    expect(parseInline('blank', [{ text: '  ', target: 't' }])).toEqual([{ kind: 'text', text: 'blank' }]);
  });
});

describe('parseMarkdown', () => {
  const kinds = (b: Block[]): string[] => b.map((x) => x.kind);

  it('reads paragraphs, headings, lists, continuation lines and code fences', () => {
    const md = '# Title\nfirst line\nsecond line\n\n- one\n  more of one\n* two\n1. a\n2) b\n\n```\n<b>code</b>\n```\nafter';
    const b = parseMarkdown(md, cites);
    expect(kinds(b)).toEqual(['h', 'p', 'ul', 'ol', 'code', 'p']);
    expect(b[0]).toEqual({ kind: 'h', level: 1, inlines: [{ kind: 'text', text: 'Title' }] });
    expect(b[1]).toEqual({ kind: 'p', inlines: [{ kind: 'text', text: 'first line second line' }] });
    expect(b[2]).toEqual({ kind: 'ul', items: [[{ kind: 'text', text: 'one more of one' }], [{ kind: 'text', text: 'two' }]] });
    expect(b[3]).toEqual({ kind: 'ol', items: [[{ kind: 'text', text: 'a' }], [{ kind: 'text', text: 'b' }]] });
    expect(b[4]).toEqual({ kind: 'code', text: '<b>code</b>' });
  });

  it('shows an unclosed fence (still streaming) as code and switches list kinds', () => {
    expect(parseMarkdown('text\n```\npartial')).toEqual([{ kind: 'p', inlines: [{ kind: 'text', text: 'text' }] }, { kind: 'code', text: 'partial' }]);
    expect(kinds(parseMarkdown('- a\n1. b\nplain'))).toEqual(['ul', 'ol', 'p']);
    expect(parseMarkdown('\r\n\r\n')).toEqual([]);
  });

  it('cuts runaway answers', () => {
    const b = parseMarkdown('x'.repeat(MAX_RENDER_CHARS + 50));
    const first = b[0] as { inlines: { text: string }[] };
    expect(first.inlines[0]?.text.length).toBe(MAX_RENDER_CHARS + 1);
    expect(first.inlines[0]?.text.endsWith('…')).toBe(true);
  });
});

describe('IncrementalMarkdown', () => {
  const answer = [
    '## Findings',
    '',
    'The order service crashed (event 12) after `level >= error` spikes.',
    '',
    '- memory grew on **WEB02**',
    '- see more',
    '  and more',
    '',
    '```',
    'stack line 1',
    '',
    'stack line 2',
    '```',
    '',
    '1. restart',
    '2. watch',
    '',
    'Done.',
  ].join('\n');

  it('gives the same blocks as a full parse at every point of a stream', () => {
    const md = new IncrementalMarkdown();
    for (let n = 0; n <= answer.length; n += 3) expect(md.parse(answer.slice(0, n), cites)).toEqual(parseMarkdown(answer.slice(0, n), cites));
    expect(md.parse(answer, cites)).toEqual(parseMarkdown(answer, cites));
  });

  it('parses only the open tail again and keeps the closed blocks', () => {
    const md = new IncrementalMarkdown();
    const first = md.parse(answer.slice(0, answer.indexOf('```')), cites);
    const more = md.parse(answer.slice(0, answer.indexOf('stack line 2')), cites);
    expect(more[0]).toBe(first[0]);
    expect(more[1]).toBe(first[1]);
    expect(md.lastParsed).toBeLessThan(40);
    md.parse(answer, cites);
    md.parse(answer + ' Really.', cites);
    expect(md.lastParsed).toBe('Done. Really.'.length);
  });

  it('starts over when the text does not extend the previous one or the citations change', () => {
    const md = new IncrementalMarkdown();
    md.parse(answer, cites);
    expect(md.parse('Other\n\ntext', cites)).toEqual(parseMarkdown('Other\n\ntext', cites));
    const other: AssistantCitation[] = [];
    expect(md.parse(answer, other)).toEqual(parseMarkdown(answer, other));
    expect(md.parse('a\r\n\r\nb', other)).toEqual(parseMarkdown('a\r\n\r\nb', other));
  });

  it('finds closed boundaries outside code fences only', () => {
    expect(closedUpTo('a\n\nb')).toBe(3);
    expect(closedUpTo('```\n\n')).toBe(0);
    expect(closedUpTo('```\n\n```\n\nx')).toBe(10);
    expect(closedUpTo('no newline')).toBe(0);
  });
});
