import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { provideShellTesting, render } from '@decryp7/paddock-ui/shell/testing';
import { PdAssistantRichText, type CitationOpened } from './rich-text';

const md = ['## Findings', '', 'See event 12 and `level >= error`; **bold** and *italic* and `code`.', '', '- one', '- two', '', '1. first', '2. second', '', '```', '<b>raw</b>', '```'].join('\n');

describe('PdAssistantRichText', () => {
  it('draws the Markdown subset with text nodes only, and cited phrases as buttons', async () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: provideShellTesting() });
    const r = await render(PdAssistantRichText, { text: md, citations: [{ text: 'event 12', target: 'record:12', title: 'Event 12' }, { text: 'level >= error', target: 'query:1' }] });
    const opened: CitationOpened[] = [];
    r.component.cite.subscribe((c) => opened.push(c));
    expect(r.one('[role=heading]').getAttribute('aria-level')).toBe('4');
    expect(r.all('ul li').map((e) => e.textContent)).toEqual(['one', 'two']);
    expect(r.all('ol li')).toHaveLength(2);
    expect(r.one('pre code').textContent).toBe('<b>raw</b>');
    expect(r.element.querySelector('b')).toBeNull();
    expect(r.one('strong').textContent).toBe('bold');
    expect(r.one('em').textContent).toBe('italic');
    expect(r.all('code').some((c) => c.textContent === 'code')).toBe(true);
    const cites = r.all('[data-testid=assistant-cite]');
    expect(cites.map((c) => c.textContent)).toEqual(['event 12', 'level >= error']);
    expect(cites[0]?.getAttribute('title')).toBe('Open Event 12');
    cites[0]?.click();
    cites[1]?.click();
    expect(opened).toEqual([{ target: 'record:12', title: 'Event 12' }, { target: 'query:1', title: 'level >= error' }]);
    await r.set({ text: 'plain' });
    expect(r.all('ul')).toHaveLength(0);
  });
});
