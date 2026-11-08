import { MockBackend } from '../../core/mock-backend';
import { button, render, settle } from '../../../testing/render';
import { PlaygroundPage, statsText } from './playground-page';
import { resetChatBackend } from './chat-backend';

const wait = (ms = 0) => new Promise((r) => setTimeout(r, ms));

describe('statsText', () => {
  it('lists TTFT, speed and tokens, plus the cold start when there was one', () => {
    const base = { promptTokens: 10, completionTokens: 1234, ttftMs: 214, genTps: 61.84, coldStartMs: null };
    expect(statsText(base)).toBe('TTFT 214 ms · 61.8 tok/s · 1,234 tokens');
    expect(statsText({ ...base, coldStartMs: 2100 })).toBe('TTFT 214 ms · 61.8 tok/s · 1,234 tokens · cold start 2.1 s');
  });
});

describe('PlaygroundPage', () => {
  const backend = (o = {}) => new MockBackend({ streamDelayMs: 0, latencyMs: 0, ...o });
  const box = (el: HTMLElement) => el.querySelector<HTMLTextAreaElement>('#pg-prompt')!;
  function type(el: HTMLElement, text: string) {
    box(el).value = text;
    box(el).dispatchEvent(new Event('input'));
  }
  async function until(fixture: Parameters<typeof settle>[0], el: HTMLElement, done: () => boolean) {
    for (let i = 0; i < 300 && !done(); i++) {
      await wait(0);
      fixture?.detectChanges();
    }
    await settle(fixture);
  }
  /** Opens the Compare tab (Chat is the first one). */
  const tabsOf = (el: HTMLElement) => Array.from(el.querySelectorAll<HTMLElement>('[role=tab]'));
  async function compare(fixture: Parameters<typeof settle>[0], el: HTMLElement) {
    await until(fixture, el, () => tabsOf(el).length === 2);
    tabsOf(el).find((t) => t.textContent?.includes('Compare'))!.click();
    await until(fixture, el, () => !!el.querySelector('[data-testid=pg-compare]'));
  }
  /** The page on its Compare tab; `el` is that tab's body (the chat tab stays in the page, with buttons of its own). */
  async function renderCompare(be: ReturnType<typeof backend>) {
    const r = await render(PlaygroundPage, be);
    await compare(r.fixture, r.el);
    return { ...r, el: r.el.querySelector<HTMLElement>('[data-testid=pg-compare]')!.parentElement! };
  }
  const finished = (el: HTMLElement) => () => el.querySelectorAll('.turn.bot').length > 0 && el.querySelectorAll('.caret, .thinking').length === 0 && !!el.querySelector('.stats');

  it('opens on the Chat tab with the assistant pane; Compare is the second tab', async () => {
    resetChatBackend();
    const { fixture, el } = await render(PlaygroundPage, backend());
    await until(fixture, el, () => tabsOf(el).length === 2);
    const tabs = tabsOf(el);
    expect(tabs.map((t) => t.textContent?.trim())).toEqual(['Chat', 'Compare']);
    expect(tabs[0]?.getAttribute('aria-selected')).toBe('true');
    expect(el.querySelector('pw-playground-chat pd-assistant')).not.toBeNull();
    expect(el.querySelector('.pane')).toBeNull();
    expect(el.querySelector('[data-testid=pg-compare]')).toBeNull();
    await compare(fixture, el);
    expect(tabs[1]?.getAttribute('aria-selected')).toBe('true');
    expect(el.querySelectorAll('[data-testid=pg-compare] .pane')).toHaveLength(2);
    // The chat stays where it was (its conversation and a running turn are kept while Compare is open).
    expect(el.querySelector('pw-playground-chat pd-assistant')).not.toBeNull();
  });

  it('is the Playground as before in a build without the chat: no tabs, one model, the Compare toggle', async () => {
    const { fixture, el } = await render(PlaygroundPage, backend({ chat: false }));
    await until(fixture, el, () => el.querySelectorAll('.pane').length > 0);
    expect(el.querySelectorAll('[role=tab]')).toHaveLength(0);
    expect(el.querySelector('pw-playground-chat')).toBeNull();
    expect(el.querySelectorAll('.pane')).toHaveLength(1);
    expect(box(el).placeholder).toBe('Message the model…');
    const toggle = button(el, 'Compare');
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    toggle.click();
    await settle(fixture);
    expect(el.querySelectorAll('.pane')).toHaveLength(2);
    expect(toggle.getAttribute('aria-pressed')).toBe('true');
  });

  it('treats a build that cannot say whether it has the chat as one without it', async () => {
    const { fixture, el } = await render(PlaygroundPage, backend({ failing: ['chatEnabled'] }));
    await until(fixture, el, () => el.querySelectorAll('.pane').length > 0);
    expect(el.querySelectorAll('[role=tab]')).toHaveLength(0);
  });

  it('shows the empty state with suggestions and a disabled Send', async () => {
    const { el } = await renderCompare(backend());
    expect(el.querySelector('.empty')?.textContent).toContain('Ask anything');
    expect(el.querySelectorAll('.sugg .chip')).toHaveLength(3);
    expect(button(el, 'Send').disabled).toBe(true);
    expect(button(el, 'Clear').disabled).toBe(true);
    expect(el.querySelectorAll('.pane')).toHaveLength(2);
    expect(el.querySelector('.pane-head .status')).not.toBeNull();
    expect(el.textContent).toContain('Hot');
  });

  it('tells the user to download a model when there is no chat model', async () => {
    const be = backend();
    be.localModels = () => Promise.resolve([]);
    const { el } = await renderCompare(be);
    expect(el.textContent).toContain('Download a chat model in Models first.');
    expect(el.querySelector('.sugg')).toBeNull();
    expect(button(el, 'Send').disabled).toBe(true);
  });

  it('shows a retryable error when the models cannot be loaded', async () => {
    const { el } = await renderCompare(backend({ failing: ['localModels'] }));
    expect(el.textContent).toContain('not responding');
  });

  it('sends a typed prompt, streams the answer and shows the stats', async () => {
    const { fixture, el } = await renderCompare(backend());
    type(el, 'Hello there');
    await settle(fixture);
    expect(button(el, 'Send').disabled).toBe(false);
    button(el, 'Send').click();
    await settle(fixture);
    expect(box(el).value).toBe('');
    expect(el.querySelector('.turn.user')?.textContent).toContain('Hello there');
    await until(fixture, el, finished(el));
    expect(el.querySelector('.turn.bot .text')?.textContent).toContain('Pitwall keeps models hot');
    expect(el.querySelector('.stats')?.textContent).toMatch(/TTFT .* tok\/s .* tokens/);
    expect(button(el, 'Clear').disabled).toBe(false);
    expect(el.querySelector('.empty')).toBeNull();
  });

  it('sends on Enter but not on Shift+Enter', async () => {
    const { fixture, el } = await renderCompare(backend());
    type(el, 'line');
    box(el).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', shiftKey: true, bubbles: true }));
    await settle(fixture);
    expect(el.querySelector('.turn.user')).toBeNull();
    const ev = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
    box(el).dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
    await settle(fixture);
    expect(el.querySelector('.turn.user')?.textContent).toContain('line');
    await until(fixture, el, finished(el));
  });

  it('does not send blank text on Enter', async () => {
    const { fixture, el } = await renderCompare(backend());
    type(el, '   ');
    box(el).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await settle(fixture);
    expect(el.querySelector('.turn')).toBeNull();
    expect(box(el).value).toBe('   ');
  });

  it('starts a conversation from a suggestion', async () => {
    const { fixture, el } = await renderCompare(backend());
    (el.querySelector('.sugg .chip') as HTMLButtonElement).click();
    await until(fixture, el, finished(el));
    expect(el.querySelector('.turn.user')?.textContent).toContain('KV cache');
  });

  it('stops a running answer and marks it stopped', async () => {
    const { fixture, el } = await renderCompare(backend({ streamDelayMs: 3 }));
    type(el, 'go');
    await settle(fixture);
    button(el, 'Send').click();
    await settle(fixture);
    await wait(30);
    fixture.detectChanges();
    button(el, 'Stop').click();
    await settle(fixture);
    expect(el.textContent).toContain('Stopped');
    expect(el.querySelector('.caret')).toBeNull();
    expect(button(el, 'Send')).toBeTruthy();
  });

  it('clears the conversation', async () => {
    const { fixture, el } = await renderCompare(backend());
    (el.querySelector('.sugg .chip') as HTMLButtonElement).click();
    await until(fixture, el, finished(el));
    button(el, 'Clear').click();
    await settle(fixture);
    expect(el.querySelector('.turn')).toBeNull();
    expect(el.querySelector('.empty')).not.toBeNull();
  });

  it('compares two models with one prompt', async () => {
    const { fixture, el } = await renderCompare(backend());
    expect(el.querySelectorAll('.pane')).toHaveLength(2);
    expect(box(el).placeholder).toBe('Message both models…');
    expect(el.querySelector('select')).toBeNull();
    for (const [id, name] of [['pg-model-0', 'Qwen3.5 9B'], ['pg-model-1', 'Qwen3.5 4B']] as const) {
      el.querySelector<HTMLElement>(`#${id}`)!.click();
      await settle(fixture);
      const opts = Array.from(Array.from(document.querySelectorAll<HTMLElement>('.mat-mdc-select-panel')).at(-1)!.querySelectorAll<HTMLElement>('mat-option'));
      opts.find((o) => o.textContent!.includes(name))!.click();
      await settle(fixture);
    }
    type(el, 'both');
    await settle(fixture);
    button(el, 'Send').click();
    await until(fixture, el, () => el.querySelectorAll('.stats').length === 2);
    expect(el.querySelectorAll('.turn.user')).toHaveLength(2);
    const stats = Array.from(el.querySelectorAll('.stats')).map((s) => s.textContent);
    expect(stats[0]).toContain('61.8');
    expect(stats[1]).toContain('118');
    expect(el.querySelector('.sugg')).toBeNull();
  });

  it('shows the error from a failed stream inline', async () => {
    const { fixture, el } = await renderCompare(backend({ failing: ['chat'] }));
    type(el, 'hi');
    await settle(fixture);
    button(el, 'Send').click();
    await until(fixture, el, () => !!el.querySelector('.err'));
    expect(el.querySelector('.err')?.textContent).toContain('not responding');
    expect(button(el, 'Send')).toBeTruthy();
  });

  it('feeds system prompt, temperature and max tokens into the store', async () => {
    const { fixture, el } = await renderCompare(backend());
    const sys = el.querySelector<HTMLTextAreaElement>('#pg-system')!;
    sys.value = 'Be terse';
    sys.dispatchEvent(new Event('input'));
    const temp = el.querySelector<HTMLInputElement>('#pg-temp')!;
    temp.value = '1.25';
    temp.dispatchEvent(new Event('input'));
    const max = el.querySelector<HTMLInputElement>('#pg-max')!;
    max.value = '64';
    max.dispatchEvent(new Event('change'));
    await settle(fixture);
    expect(el.querySelector('output')?.textContent).toBe('1.25');
    expect(sys.value).toBe('Be terse');
    expect(max.value).toBe('64');
    // Rule 47: the fields are Material, the temperature a Material slider.
    expect(temp.closest('mat-slider')).not.toBeNull();
    for (const f of [sys, max, box(el)]) expect(f.closest('mat-form-field')).not.toBeNull();
  });
});
