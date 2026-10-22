import { MockBackend } from '../../core/mock-backend';
import { button, render, settle } from '../../../testing/render';
import { PlaygroundPage, statsText } from './playground-page';

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
  const finished = (el: HTMLElement) => () => el.querySelectorAll('.turn.bot').length > 0 && el.querySelectorAll('.caret, .thinking').length === 0 && !!el.querySelector('.stats');

  it('shows the empty state with suggestions and a disabled Send', async () => {
    const { el } = await render(PlaygroundPage, backend());
    expect(el.querySelector('.empty')?.textContent).toContain('Ask anything');
    expect(el.querySelectorAll('.sugg .chip')).toHaveLength(3);
    expect(button(el, 'Send').disabled).toBe(true);
    expect(button(el, 'Clear').disabled).toBe(true);
    expect(el.querySelectorAll('.pane')).toHaveLength(1);
    expect(el.querySelector('.pane-head .status')).not.toBeNull();
    expect(el.textContent).toContain('Hot');
  });

  it('tells the user to download a model when there is no chat model', async () => {
    const be = backend();
    be.localModels = () => Promise.resolve([]);
    const { el } = await render(PlaygroundPage, be);
    expect(el.textContent).toContain('Download a chat model in Models first.');
    expect(el.querySelector('.sugg')).toBeNull();
    expect(button(el, 'Send').disabled).toBe(true);
  });

  it('shows a retryable error when the models cannot be loaded', async () => {
    const { el } = await render(PlaygroundPage, backend({ failing: ['localModels'] }));
    expect(el.textContent).toContain('not responding');
  });

  it('sends a typed prompt, streams the answer and shows the stats', async () => {
    const { fixture, el } = await render(PlaygroundPage, backend());
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
    const { fixture, el } = await render(PlaygroundPage, backend());
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
    const { fixture, el } = await render(PlaygroundPage, backend());
    type(el, '   ');
    box(el).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await settle(fixture);
    expect(el.querySelector('.turn')).toBeNull();
    expect(box(el).value).toBe('   ');
  });

  it('starts a conversation from a suggestion', async () => {
    const { fixture, el } = await render(PlaygroundPage, backend());
    (el.querySelector('.sugg .chip') as HTMLButtonElement).click();
    await until(fixture, el, finished(el));
    expect(el.querySelector('.turn.user')?.textContent).toContain('KV cache');
  });

  it('stops a running answer and marks it stopped', async () => {
    const { fixture, el } = await render(PlaygroundPage, backend({ streamDelayMs: 3 }));
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
    const { fixture, el } = await render(PlaygroundPage, backend());
    (el.querySelector('.sugg .chip') as HTMLButtonElement).click();
    await until(fixture, el, finished(el));
    button(el, 'Clear').click();
    await settle(fixture);
    expect(el.querySelector('.turn')).toBeNull();
    expect(el.querySelector('.empty')).not.toBeNull();
  });

  it('compares two models with one prompt', async () => {
    const { fixture, el } = await render(PlaygroundPage, backend());
    const toggle = button(el, 'Compare');
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    toggle.click();
    await settle(fixture);
    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    expect(el.querySelectorAll('.pane')).toHaveLength(2);
    expect(box(el).placeholder).toBe('Message both models…');
    const selects = el.querySelectorAll<HTMLSelectElement>('select.model');
    selects[0]!.value = 'qwen3-8b-instruct-q4_k_m';
    selects[0]!.dispatchEvent(new Event('change'));
    selects[1]!.value = 'llama-3.2-3b-instruct-q5_k_m';
    selects[1]!.dispatchEvent(new Event('change'));
    await settle(fixture);
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
    const { fixture, el } = await render(PlaygroundPage, backend({ failing: ['chat'] }));
    type(el, 'hi');
    await settle(fixture);
    button(el, 'Send').click();
    await until(fixture, el, () => !!el.querySelector('.err'));
    expect(el.querySelector('.err')?.textContent).toContain('not responding');
    expect(button(el, 'Send')).toBeTruthy();
  });

  it('feeds system prompt, temperature and max tokens into the store', async () => {
    const { fixture, el } = await render(PlaygroundPage, backend());
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
  });
});
