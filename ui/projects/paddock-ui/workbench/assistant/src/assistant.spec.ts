import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { msg } from '@decryp7/paddock-ui/shell';
import { MemoryPitwallBackend, PitwallBackend, providePitwall } from '@decryp7/paddock-ui/shell/pitwall';
import { provideShellTesting, render, settle } from '@decryp7/paddock-ui/shell/testing';
import { defineAssistantContext, provideWorkbenchContributions } from '@decryp7/paddock-ui/workbench';
import { FIXTURE_CONVERSATION } from '../generated/assistant-fixtures';
import { PdAssistant } from './assistant';
import { AssistantBackend, MemoryAssistantBackend, type MemoryAssistantOptions } from './backend';
import type { ToolCard } from './chat-model';
import { provideAssistantRenderers, type AssistantCitationHandler } from './tool-cards';
import type { CitationOpened } from './rich-text';
import { assertStatusGlyphs } from '@decryp7/paddock-ui/testing';

@Component({ selector: 'pd-test-card', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p data-testid="custom-card">{{ card().name }} drawn by the app</p>` })
class CustomCard {
  readonly card = input.required<ToolCard>();
}

interface Options extends MemoryAssistantOptions {
  readonly pitwall?: MemoryPitwallBackend | null;
  readonly handler?: AssistantCitationHandler;
  readonly custom?: boolean;
}

async function open(options: Options = {}, inputs: Readonly<Record<string, unknown>> = {}) {
  const backend = new MemoryAssistantBackend(options);
  const pitwall = options.pitwall === undefined ? new MemoryPitwallBackend({ state: { kind: 'ready', model: 'q', vision: false, contextTokens: 8192 } }) : options.pitwall;
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: [
      ...provideShellTesting({
        context: () => Promise.resolve(() => ({})),
        providers: [
          { provide: AssistantBackend, useValue: backend },
          ...(pitwall ? [{ provide: PitwallBackend, useValue: pitwall }, providePitwall({ backend: () => pitwall })] : []),
          provideAssistantRenderers({ tools: options.custom ? [{ tool: 'find', component: CustomCard }] : [], ...(options.handler ? { citations: options.handler } : {}) }),
          provideWorkbenchContributions({
            assistantContext: [defineAssistantContext({ id: 'wd.sel', label: msg('wd.sel', 'Selection'), icon: 'pd:info', value: () => ({ text: 'main.rs:12', detail: '12-14' }) })],
          }),
        ],
      }),
    ],
  });
  const r = await render(PdAssistant, inputs);
  await settle(r.fixture);
  return { r, backend, pitwall };
}

const type = async (r: Awaited<ReturnType<typeof open>>['r'], text: string): Promise<HTMLTextAreaElement> => {
  const box = r.one<HTMLTextAreaElement>('[data-testid=assistant-input]');
  box.value = text;
  box.dispatchEvent(new Event('input'));
  await settle(r.fixture);
  return box;
};

const press = async (r: Awaited<ReturnType<typeof open>>['r'], box: HTMLElement, init: KeyboardEventInit): Promise<KeyboardEvent> => {
  const e = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  box.dispatchEvent(e);
  for (let i = 0; i < 5; i++) await settle(r.fixture);
  return e;
};

const text = (r: Awaited<ReturnType<typeof open>>['r'], testid: string): string => r.all(`[data-testid=${testid}]`).map((e) => e.textContent?.trim()).join('|');

describe('PdAssistant', () => {
  it('shows an empty conversation calmly, with the composer, the context chip and no stray controls', async () => {
    const { r } = await open({ info: { provider: 'p', tools: [], hasWrite: false } });
    expect(text(r, 'assistant-empty')).toContain('Ask a question');
    expect(r.one('[data-testid=assistant-input]').getAttribute('aria-label')).toBe('Your question');
    expect(r.all('[data-testid=assistant-chip]')).toHaveLength(1);
    expect(r.one('[data-testid=assistant-chip]').textContent).toContain('Selection');
    expect(r.all('[data-testid=assistant-allow-actions]')).toHaveLength(0);
    expect(r.one<HTMLButtonElement>('[data-testid=assistant-send]').disabled).toBe(true);
    expect(r.all('[data-testid=needs-ai]')).toHaveLength(0);
  });

  it('sends with Enter, adds a line with Shift+Enter and draws the answer as text, never as markup', async () => {
    const { r, backend } = await open({ reply: () => 'Look: <img src=x onerror=alert(1)> and **bold**' });
    const box = await type(r, 'hello');
    const shift = await press(r, box, { key: 'Enter', shiftKey: true });
    expect(shift.defaultPrevented).toBe(false);
    expect(backend.sent).toHaveLength(0);
    const enter = await press(r, box, { key: 'Enter' });
    expect(enter.defaultPrevented).toBe(true);
    expect(backend.sent[0]?.text).toBe('hello');
    expect(backend.sent[0]?.context[0]).toMatchObject({ id: 'wd.sel', label: 'Selection: 12-14', text: 'main.rs:12' });
    expect(text(r, 'assistant-user')).toContain('hello');
    expect(r.one('[data-testid=assistant-answer]').textContent).toContain('<img src=x onerror=alert(1)>');
    expect(r.element.querySelector('img')).toBeNull();
    expect(r.one('[data-testid=assistant-answer] strong').textContent).toBe('bold');
    expect(r.one<HTMLTextAreaElement>('[data-testid=assistant-input]').value).toBe('');
  });

  it('sends with the button, does nothing for Enter during composition, and removes a chip before sending', async () => {
    const { r, backend } = await open();
    const box = await type(r, 'q');
    await press(r, box, { key: 'Enter', isComposing: true });
    expect(backend.sent).toHaveLength(0);
    r.one('[data-testid=assistant-chip]').click();
    await settle(r.fixture);
    expect(r.one('[data-testid=assistant-chip]').getAttribute('aria-pressed')).toBe('false');
    r.one('[data-testid=assistant-send]').click();
    for (let i = 0; i < 5; i++) await settle(r.fixture);
    expect(backend.sent[0]?.context).toEqual([]);
  });

  it('labels the context chip the same way in the composer and in the sent question (P3-5)', async () => {
    const { r } = await open();
    const inComposer = r.one('[data-testid=assistant-chip] .chip-label').textContent?.trim();
    r.one('[data-testid=assistant-send]');
    const box = await type(r, 'q');
    await press(r, box, { key: 'Enter' });
    for (let i = 0; i < 6; i++) await settle(r.fixture);
    expect(r.one('[data-testid=assistant-user] .chip').textContent?.trim()).toBe(inComposer);
  });

  it('stops a running answer with Escape or the Stop button and says so', async () => {
    const { r, backend } = await open({ reply: () => 'one two three' });
    backend.hold();
    const box = await type(r, 'go');
    await press(r, box, { key: 'Enter' });
    expect(r.one('[data-testid=assistant-stop]')).toBeTruthy();
    expect(r.all('[data-testid=assistant-send]')).toHaveLength(0);
    expect(r.one('[data-testid=assistant-log]').getAttribute('aria-busy')).toBe('true');
    const esc = await press(r, box, { key: 'Escape' });
    expect(esc.defaultPrevented).toBe(true);
    expect(text(r, 'assistant-stopped')).toBe('Stopped.');
    expect(r.all('[data-testid=assistant-send]')).toHaveLength(1);
    // Escape with nothing running is left to the page.
    expect((await press(r, box, { key: 'Escape' })).defaultPrevented).toBe(false);
    backend.hold();
    await type(r, 'again');
    r.one('[data-testid=assistant-send]').click();
    for (let i = 0; i < 5; i++) await settle(r.fixture);
    r.one('[data-testid=assistant-stop]').click();
    for (let i = 0; i < 5; i++) await settle(r.fixture);
    expect(r.all('[data-testid=assistant-stopped]')).toHaveLength(2);
  });

  it('draws tool calls as cards (the generic one, or the app\'s own) and cited phrases as buttons', async () => {
    const opened: CitationOpened[] = [];
    const { r } = await open({ handler: { open: (c) => void opened.push(c) } });
    const box = await type(r, '/tool things');
    await press(r, box, { key: 'Enter' });
    const card = r.one('[data-testid=assistant-tool-card]');
    expect(card.getAttribute('data-state')).toBe('ok');
    expect(card.textContent).toContain('find');
    expect(text(r, 'assistant-tool-result')).toBe('2 things found');
    const cite = r.one('[data-testid=assistant-cite]');
    expect(cite.textContent).toBe('item 7');
    cite.click();
    expect(opened).toEqual([{ target: 'record:7', title: 'Item 7' }]);

    const custom = await open({ custom: true });
    await press(custom.r, await type(custom.r, '/tool x'), { key: 'Enter' });
    expect(text(custom.r, 'custom-card')).toBe('find drawn by the app');
    expect(custom.r.all('[data-testid=assistant-tool-card]')).toHaveLength(0);
  });

  it('hands a citation to the app\'s output when no handler is registered', async () => {
    const { r } = await open();
    const got: CitationOpened[] = [];
    r.component.citation.subscribe((c) => got.push(c));
    await press(r, await type(r, '/tool x'), { key: 'Enter' });
    r.one('[data-testid=assistant-cite]').click();
    expect(got).toEqual([{ target: 'record:7', title: 'Item 7' }]);
  });

  it('asks before a write: the prompt shows the call and Allow runs it, Decline does not', async () => {
    const { r, backend } = await open();
    expect(r.one('[data-testid=assistant-allow-actions]')).toBeTruthy();
    const check = r.one<HTMLInputElement>('[data-testid=assistant-allow-actions]');
    check.checked = true;
    check.dispatchEvent(new Event('change'));
    await settle(r.fixture);
    await press(r, await type(r, '/write'), { key: 'Enter' });
    expect(text(r, 'assistant-approval')).toContain('Erase everything');
    expect(r.one('[data-testid=assistant-tool-card]').getAttribute('data-state')).toBe('awaiting');
    r.one('[data-testid=assistant-allow]').click();
    for (let i = 0; i < 6; i++) await settle(r.fixture);
    expect(backend.erased).toBe(1);
    expect(r.all('[data-testid=assistant-approval]')).toHaveLength(0);
    await press(r, await type(r, '/write'), { key: 'Enter' });
    r.one('[data-testid=assistant-decline]').click();
    for (let i = 0; i < 6; i++) await settle(r.fixture);
    expect(backend.erased).toBe(1);
  });

  it('lists, opens and deletes conversations', async () => {
    const { r } = await open({ conversations: [FIXTURE_CONVERSATION] });
    expect(r.all('[data-testid=assistant-list]')).toHaveLength(0);
    r.one('[data-testid=assistant-list-toggle]').click();
    await settle(r.fixture);
    expect(r.one('[data-testid=assistant-open]').textContent).toContain('Why did the build fail?');
    r.one('[data-testid=assistant-open]').click();
    for (let i = 0; i < 4; i++) await settle(r.fixture);
    expect(text(r, 'assistant-user')).toContain('Why did the build fail?');
    expect(r.one('[data-testid=assistant-answer]').textContent).toContain('linker ran out of memory');
    expect(r.all('[data-testid=assistant-list]')).toHaveLength(0);
    r.one('[data-testid=assistant-list-toggle]').click();
    await settle(r.fixture);
    expect(text(r, 'assistant-title')).toBe('Why did the build fail?');
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    r.one('[data-testid=assistant-delete]').click();
    await settle(r.fixture);
    expect(r.one('[data-testid=assistant-list]').textContent).toContain('No conversations yet.');
    expect(text(r, 'assistant-undo')).toContain('Deleted "Why did the build fail?"');
    expect(text(r, 'assistant-title')).toBe('New conversation');
    expect(text(r, 'assistant-turn')).toBe('');
    r.one('[data-testid=assistant-undo-button]').click();
    for (let i = 0; i < 4; i++) await settle(r.fixture);
    expect(text(r, 'assistant-title')).toBe('Why did the build fail?');
    expect(text(r, 'assistant-user')).toContain('Why did the build fail?');
    r.one('[data-testid=assistant-list-toggle]').click();
    await settle(r.fixture);
    r.one('[data-testid=assistant-delete]').click();
    await settle(r.fixture);
    await vi.advanceTimersByTimeAsync(8000);
    vi.useRealTimers();
    for (let i = 0; i < 4; i++) await settle(r.fixture);
    expect(text(r, 'assistant-turn')).toBe('');
    expect(r.all('[data-testid=assistant-undo]')).toHaveLength(0);
    expect(text(r, 'assistant-title')).toBe('New conversation');
    r.one('[data-testid=assistant-new]').click();
    await settle(r.fixture);
    expect(r.all('[data-testid=assistant-turn]')).toHaveLength(0);
  });

  it('shows the shared needs-AI state and keeps the composer from sending until Pitwall is ready', async () => {
    const pitwall = new MemoryPitwallBackend({ state: { kind: 'notRunning' }, app: 'Demo' });
    const { r, backend } = await open({ pitwall });
    expect(r.one('[data-testid=needs-ai-message]').textContent).toContain('not running');
    const box = await type(r, 'hello');
    expect(r.one<HTMLButtonElement>('[data-testid=assistant-send]').disabled).toBe(true);
    expect((await press(r, box, { key: 'Enter' })).defaultPrevented).toBe(true);
    expect(backend.sent).toHaveLength(0);
    pitwall.setState({ kind: 'ready', model: 'q', vision: false, contextTokens: 8192 });
    await settle(r.fixture);
    await settle(r.fixture);
    expect(r.all('[data-testid=needs-ai]')).toHaveLength(0);
    expect(r.one<HTMLButtonElement>('[data-testid=assistant-send]').disabled).toBe(false);
    // Its steps and the settings link come out as outputs for the app.
    const steps: string[] = [];
    r.component.step.subscribe((s) => steps.push(s));
    r.component.openSettings.subscribe(() => steps.push('settings'));
    pitwall.setState({ kind: 'notRunning' });
    await settle(r.fixture);
    await settle(r.fixture);
    r.one('[data-testid=needs-ai-startPitwall]').click();
    expect(steps).toEqual(['startPitwall']);
  });

  it('works without the needs-AI state (a window with no Pitwall store) and shows failures with a way out', async () => {
    const { r } = await open({ pitwall: null, sendError: 'The assistant is answering already.' });
    expect(r.all('pd-needs-ai')).toHaveLength(0);
    await press(r, await type(r, 'q'), { key: 'Enter' });
    expect(text(r, 'assistant-failure')).toContain('The assistant is answering already.');
    expect(r.one<HTMLTextAreaElement>('[data-testid=assistant-input]').value).toBe('q');
    r.one('[data-testid=assistant-failure] button').click();
    await settle(r.fixture);
    expect(r.all('[data-testid=assistant-failure]')).toHaveLength(0);
  });

  it('shows a failed load with Try again', async () => {
    const backend = new MemoryAssistantBackend();
    let fail = true;
    const info = backend.info.bind(backend);
    backend.info = () => (fail ? Promise.reject(new Error('The engine is not there.')) : info());
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [...provideShellTesting({ providers: [{ provide: AssistantBackend, useValue: backend }] })] });
    const r = await render(PdAssistant);
    for (let i = 0; i < 4; i++) await settle(r.fixture);
    expect(text(r, 'assistant-load-failed')).toContain('The engine is not there.');
    fail = false;
    r.one('[data-testid=assistant-retry]').click();
    for (let i = 0; i < 4; i++) await settle(r.fixture);
    expect(r.all('[data-testid=assistant-load-failed]')).toHaveLength(0);
    expect(text(r, 'assistant-empty')).toContain('Ask a question');
  });

  it('puts the keyboard in the question box, and shows budget messages, notes and errors of a turn', async () => {
    const { r, backend } = await open();
    r.component.focusInput();
    expect(document.activeElement).toBe(r.one('[data-testid=assistant-input]'));
    await press(r, await type(r, 'q'), { key: 'Enter' });
    backend.emit('m1', { type: 'notice', message: 'A call could not be read.' });
    backend.emit('m1', { type: 'done', text: 'x', usage: { inputTokens: 0, outputTokens: 0 }, end: 'budgetTime', message: 'The assistant ran out of time.' });
    await settle(r.fixture);
    expect(text(r, 'assistant-note')).toContain('A call could not be read.');
    expect(text(r, 'assistant-budget')).toBe('The assistant ran out of time.');
  });

  describe('review fixes', () => {
    afterEach(() => vi.useRealTimers());

    it('shows New conversation as the heading and a chat icon via history toggle with a tooltip', async () => {
      const { r } = await open();
      expect(text(r, 'assistant-title')).toBe('New conversation');
      expect(r.one('[data-testid=assistant-list-toggle]').getAttribute('title')).toBe('Conversations');
    });

    it('undoes a delete from the toast', async () => {
      const { r, backend } = await open({ conversations: [FIXTURE_CONVERSATION] });
      r.one('[data-testid=assistant-list-toggle]').click();
      await settle(r.fixture);
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
      r.one('[data-testid=assistant-delete]').click();
      await settle(r.fixture);
      expect(r.all('[data-testid=assistant-open]')).toHaveLength(0);
      r.one('[data-testid=assistant-undo-button]').click();
      await settle(r.fixture);
      expect(r.all('[data-testid=assistant-open]')).toHaveLength(1);
      await vi.advanceTimersByTimeAsync(20000);
      expect((await backend.conversations()).length).toBe(1);
    });

    it('keeps the draft and disables the composer while Pitwall is not running, with one primary', async () => {
      const pitwall = new MemoryPitwallBackend({ state: { kind: 'notRunning' }, app: 'Demo' });
      const { r } = await open({ pitwall, info: { provider: 'p', tools: [], hasWrite: true } });
      const box = r.one<HTMLTextAreaElement>('[data-testid=assistant-input]');
      expect(box.disabled).toBe(true);
      expect(box.placeholder).toBe('Start Pitwall to ask questions');
      expect(r.one<HTMLInputElement>('[data-testid=assistant-allow-actions]').disabled).toBe(true);
      expect(r.one<HTMLButtonElement>('[data-testid=assistant-chip]').disabled).toBe(true);
      expect(r.one('[data-testid=assistant-send]').classList.contains('pd-btn--primary')).toBe(false);
      expect(r.element.querySelectorAll('.pd-btn--primary')).toHaveLength(1);
      // The draft written before is kept and comes back with the composer.
      pitwall.setState({ kind: 'ready', model: 'q', vision: false, contextTokens: 8192 });
      await settle(r.fixture);
      await settle(r.fixture);
      const live = r.one<HTMLTextAreaElement>('[data-testid=assistant-input]');
      expect(live.disabled).toBe(false);
      expect(live.placeholder).toContain('Enter sends');
      await type(r, 'draft');
      pitwall.setState({ kind: 'notRunning' });
      await settle(r.fixture);
      await settle(r.fixture);
      expect(r.one<HTMLTextAreaElement>('[data-testid=assistant-input]').value).toBe('draft');
    });

    it('words the composer hint after the state of Pitwall', async () => {
      const words: Record<string, string> = {
        notInstalled: 'Install Pitwall',
        needsPairing: 'Connect Pitwall',
        paused: 'Resume Pitwall',
        noSuitableModel: 'Download a model',
        incompatible: 'Update to ask',
        unavailable: 'not available',
      };
      const states = {
        notInstalled: { kind: 'notInstalled' },
        needsPairing: { kind: 'needsPairing' },
        paused: { kind: 'paused', stopped: false },
        noSuitableModel: { kind: 'noSuitableModel', needs: { vision: false, contextTokens: 8192 } },
        incompatible: { kind: 'incompatible', message: 'm', updateApp: true },
        unavailable: { kind: 'unavailable', message: 'm' },
      } as const;
      for (const [kind, state] of Object.entries(states)) {
        const { r } = await open({ pitwall: new MemoryPitwallBackend({ state: state as never, app: 'Demo' }) });
        expect(r.one<HTMLTextAreaElement>('[data-testid=assistant-input]').placeholder).toContain(words[kind]);
      }
    });

    it('follows the user’s own question to the end even when they had scrolled up (review 2 shot 13)', async () => {
      const { r, backend } = await open();
      backend.hold();
      const log = r.one('[data-testid=assistant-log]');
      let top = 0;
      Object.defineProperty(log, 'scrollHeight', { configurable: true, get: () => 1000 });
      Object.defineProperty(log, 'clientHeight', { configurable: true, get: () => 200 });
      Object.defineProperty(log, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => void (top = v) });
      top = 100;
      log.dispatchEvent(new Event('scroll'));
      await press(r, await type(r, 'next question'), { key: 'Enter' });
      backend.emit('m1', { type: 'delta', text: 'answer' });
      await settle(r.fixture);
      expect(top).toBe(1000);
      expect(r.all('[data-testid=assistant-jump]')).toHaveLength(0);
    });

    it('keeps the latest item in view when the log shrinks under a user at the bottom, such as an approval growing (review 4 P3-1)', async () => {
      const fired: ResizeObserverCallback[] = [];
      vi.stubGlobal(
        'ResizeObserver',
        class {
          constructor(cb: ResizeObserverCallback) {
            fired.push(cb);
          }
          observe(): void {}
          unobserve(): void {}
          disconnect(): void {}
        },
      );
      const { r } = await open();
      await settle(r.fixture);
      const log = r.one('[data-testid=assistant-log]');
      let top = 800;
      Object.defineProperty(log, 'scrollHeight', { configurable: true, get: () => 1000 });
      Object.defineProperty(log, 'clientHeight', { configurable: true, get: () => 200 });
      Object.defineProperty(log, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => void (top = v) });
      log.dispatchEvent(new Event('scroll'));
      // The approval's Details open: the log is shorter, its scroll position is not.
      Object.defineProperty(log, 'clientHeight', { configurable: true, get: () => 120 });
      expect(fired.length).toBeGreaterThan(0);
      for (const cb of fired) cb([], {} as ResizeObserver);
      expect(top).toBe(1000);
      // A user who scrolled up to read is left where they are.
      top = 300;
      log.dispatchEvent(new Event('scroll'));
      for (const cb of fired) cb([], {} as ResizeObserver);
      expect(top).toBe(300);
    });

    it('follows the streaming text only while the user is at the bottom, else offers Jump to latest', async () => {
      const { r, backend } = await open();
      backend.hold();
      const log = r.one('[data-testid=assistant-log]');
      let top = 0;
      Object.defineProperty(log, 'scrollHeight', { configurable: true, get: () => 1000 });
      Object.defineProperty(log, 'clientHeight', { configurable: true, get: () => 200 });
      Object.defineProperty(log, 'scrollTop', { configurable: true, get: () => top, set: (v: number) => void (top = v) });
      await press(r, await type(r, 'go'), { key: 'Enter' });
      expect(top).toBe(1000);
      expect(r.all('[data-testid=assistant-jump]')).toHaveLength(0);
      // The user scrolls up to read: new text neither moves them nor is lost.
      top = 500;
      log.dispatchEvent(new Event('scroll'));
      backend.emit('m1', { type: 'delta', text: 'more' });
      await settle(r.fixture);
      expect(top).toBe(500);
      expect(r.all('[data-testid=assistant-jump]')).toHaveLength(1);
      r.one('[data-testid=assistant-jump]').click();
      await settle(r.fixture);
      expect(top).toBe(1000);
      expect(r.all('[data-testid=assistant-jump]')).toHaveLength(0);
      // Within 24 px of the bottom counts as at the bottom, and scrolling back down hides the pill.
      top = 500;
      log.dispatchEvent(new Event('scroll'));
      backend.emit('m1', { type: 'delta', text: 'again' });
      await settle(r.fixture);
      top = 780;
      log.dispatchEvent(new Event('scroll'));
      await settle(r.fixture);
      expect(r.all('[data-testid=assistant-jump]')).toHaveLength(0);
      backend.emit('m1', { type: 'delta', text: 'last' });
      await settle(r.fixture);
      expect(top).toBe(1000);
    });

    it('does not announce streaming text, and announces once when the answer is ready, stopped or failed', async () => {
      const { r, backend } = await open();
      expect(r.one('[data-testid=assistant-log]').getAttribute('aria-live')).toBe('off');
      expect(r.one('[data-testid=assistant-announce]').getAttribute('aria-live')).toBe('polite');
      backend.hold();
      await press(r, await type(r, 'one'), { key: 'Enter' });
      backend.emit('m1', { type: 'delta', text: 'partial' });
      await settle(r.fixture);
      expect(text(r, 'assistant-announce')).toBe('');
      backend.emit('m1', { type: 'done', text: 'partial', usage: { inputTokens: 0, outputTokens: 0 }, end: 'answered' });
      await settle(r.fixture);
      expect(text(r, 'assistant-announce')).toBe('Answer ready');
      await press(r, await type(r, 'two'), { key: 'Enter' });
      expect(text(r, 'assistant-announce')).toBe('');
      backend.emit('m1', { type: 'done', text: '', usage: { inputTokens: 0, outputTokens: 0 }, end: 'cancelled' });
      await settle(r.fixture);
      expect(text(r, 'assistant-announce')).toBe('Stopped.');
      await press(r, await type(r, 'three'), { key: 'Enter' });
      backend.emit('m1', { type: 'error', kind: 'failed', message: 'The engine broke.' });
      await settle(r.fixture);
      expect(text(r, 'assistant-announce')).toBe('The engine broke.');
      await press(r, await type(r, 'four'), { key: 'Enter' });
      backend.emit('m1', { type: 'error', kind: 'failed', message: 'x', needsAi: { kind: 'notRunning' } });
      await settle(r.fixture);
      expect(text(r, 'assistant-announce')).toBe('The answer failed.');
    });

    it('shows the tool and its arguments in the approval prompt, with full-size buttons', async () => {
      const { r, backend } = await open();
      r.one<HTMLInputElement>('[data-testid=assistant-allow-actions]').click();
      await press(r, await type(r, '/write'), { key: 'Enter' });
      expect(text(r, 'assistant-approval-tool')).toBe('erase');
      expect(text(r, 'assistant-approval-args')).toBe('No arguments');
      expect(r.one('[data-testid=assistant-allow]').classList.contains('pd-btn--sm')).toBe(false);
      expect(r.one('[data-testid=assistant-decline]').classList.contains('pd-btn--sm')).toBe(false);
      backend.emit('m1', { type: 'approvalNeeded', destructive: false, remember: false, callId: 'call_2', name: 'erase', summary: 's', input: { path: '/x' } });
      await settle(r.fixture);
      expect(text(r, 'assistant-approval-args')).toContain('"path": "/x"');
      backend.emit('m1', { type: 'approvalNeeded', destructive: false, remember: false, callId: 'call_2', name: 'erase', summary: 's', input: { big: 'y'.repeat(3000) } });
      await settle(r.fixture);
      expect(text(r, 'assistant-approval-args').endsWith('…')).toBe(true);
      backend.emit('m1', { type: 'approvalNeeded', destructive: false, remember: false, callId: 'call_2', name: 'erase', summary: 's', input: undefined });
      await settle(r.fixture);
      expect(text(r, 'assistant-approval-args')).toBe('No arguments');
    });

    it('draws a chip icon only when the glyph exists', async () => {
      const { r } = await open();
      expect(r.element.querySelectorAll('[data-testid=assistant-chip] pd-glyph')).toHaveLength(2);
    });

    it('empties the box when typing and Enter happen in one task (P2-I)', async () => {
      const { r } = await open();
      const box = r.one<HTMLTextAreaElement>('[data-testid=assistant-input]');
      box.value = 'hello';
      box.dispatchEvent(new Event('input'));
      box.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
      await settle(r.fixture);
      await settle(r.fixture);
      expect(box.value).toBe('');
      expect(text(r, 'assistant-user')).toContain('hello');
    });

    it('shows a progress cue after the streaming text only while the turn runs (P2-I)', async () => {
      const { r, backend } = await open();
      backend.hold();
      await press(r, await type(r, 'q'), { key: 'Enter' });
      backend.emit('m1', { type: 'delta', text: 'partial' });
      await settle(r.fixture);
      expect(r.all('[data-testid=assistant-streaming]')).toHaveLength(1);
      backend.emit('m1', { type: 'done', text: 'partial', usage: { inputTokens: 1, outputTokens: 1 }, end: 'answered' });
      await settle(r.fixture);
      expect(r.all('[data-testid=assistant-streaming]')).toHaveLength(0);
    });

    it('makes Allow neutral and Decline the secondary on the right for a write that can be undone, and names the tool (P2-C)', async () => {
      const { r, backend } = await open();
      r.one<HTMLInputElement>('[data-testid=assistant-allow-actions]').click();
      await press(r, await type(r, '/write'), { key: 'Enter' });
      backend.emit('m1', { type: 'approvalNeeded', destructive: false, remember: false, callId: 'call_2', name: 'erase', summary: 'Erase everything', input: {} });
      await settle(r.fixture);
      const allow = r.one('[data-testid=assistant-allow]');
      const decline = r.one('[data-testid=assistant-decline]');
      expect(allow.classList.contains('pd-btn--primary')).toBe(false);
      expect(allow.classList.contains('pd-btn--danger')).toBe(false);
      expect(allow.textContent?.trim()).toBe('Allow erase');
      expect(decline.classList.contains('pd-btn--ghost')).toBe(true);
      expect(allow.compareDocumentPosition(decline) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
      expect(text(r, 'assistant-approval-consequence')).toBe('Erase everything');
      expect(r.one('[data-testid=assistant-approval]').classList.contains('danger')).toBe(false);
      expect(r.all('[data-testid=assistant-approval-final]')).toHaveLength(0);
      expect(r.element.querySelectorAll('[data-testid=assistant-approval] .pd-btn--primary')).toHaveLength(0);
    });

    it('makes a destructive approval a danger choice: Decline is the focused default, Allow a danger button, never the primary (P2-C)', async () => {
      const { r } = await open();
      r.one<HTMLInputElement>('[data-testid=assistant-allow-actions]').click();
      await press(r, await type(r, '/write'), { key: 'Enter' });
      await settle(r.fixture);
      const section = r.one('[data-testid=assistant-approval]');
      const allow = r.one('[data-testid=assistant-allow]');
      const decline = r.one('[data-testid=assistant-decline]');
      expect(section.classList.contains('danger')).toBe(true);
      expect(section.getAttribute('aria-describedby')).toBe('pd-assistant-approval-final');
      expect(text(r, 'assistant-approval-final')).toBe('This cannot be undone.');
      // Regression (review 3 sheet): a whitespace-only node between the label and the action is dropped by the compiler.
      expect(r.one('#pd-assistant-approval').textContent).toBe('The assistant wants to: Erase everything');
      expect(allow.classList.contains('pd-btn--danger')).toBe(true);
      expect(allow.classList.contains('pd-btn--primary')).toBe(false);
      expect(allow.classList.contains('pd-btn--danger-fill')).toBe(false);
      expect(decline.classList.contains('pd-btn--ghost')).toBe(false);
      expect(document.activeElement).toBe(decline);
      expect(r.element.querySelectorAll('[data-testid=assistant-approval] .pd-btn--primary')).toHaveLength(0);
    });

    it('draws a declined tool as a neutral card with a muted Declined, not as a failure (P2-G)', async () => {
      const { r } = await open();
      r.one<HTMLInputElement>('[data-testid=assistant-allow-actions]').click();
      await press(r, await type(r, '/write'), { key: 'Enter' });
      r.one('[data-testid=assistant-decline]').click();
      for (let i = 0; i < 8; i++) await settle(r.fixture);
      const card = r.one('[data-testid=assistant-tool-card]');
      expect(card.getAttribute('data-state')).toBe('declined');
      expect(text(r, 'assistant-tool-result')).toBe('Declined');
      expect(r.one('[data-testid=assistant-tool-result]').classList.contains('muted')).toBe(true);
    });

    it('draws a tool the user stopped as a neutral card with a muted Stopped, not as a failure (P2-3)', async () => {
      const { r } = await open();
      await press(r, await type(r, '/tool-slow find'), { key: 'Enter' });
      for (let i = 0; i < 8; i++) await settle(r.fixture);
      expect(r.one('[data-testid=assistant-tool-card]').getAttribute('data-state')).toBe('running');
      r.one('[data-testid=assistant-stop]').click();
      for (let i = 0; i < 8; i++) await settle(r.fixture);
      expect(r.one('[data-testid=assistant-tool-card]').getAttribute('data-state')).toBe('stopped');
      expect(text(r, 'assistant-tool-result')).toBe('Stopped');
      expect(r.one('[data-testid=assistant-tool-result]').classList.contains('muted')).toBe(true);
    });

    it('still draws a tool that failed for real as a failure with its reason (P2-G)', async () => {
      const { r } = await open();
      await press(r, await type(r, '/tool-fail'), { key: 'Enter' });
      for (let i = 0; i < 8; i++) await settle(r.fixture);
      expect(r.one('[data-testid=assistant-tool-card]').getAttribute('data-state')).toBe('failed');
      expect(text(r, 'assistant-tool-result')).toBe('The index is locked by another process.');
      assertStatusGlyphs(r.one('[data-testid=assistant-tool-card]'));
    });

    it('designs a failed turn: icon, plain words, likely cause, Retry that resends and details, in the answer column (P2-H)', async () => {
      const { r, backend } = await open();
      await press(r, await type(r, '/fail Explain it'), { key: 'Enter' });
      for (let i = 0; i < 40; i++) await settle(r.fixture);
      const error = r.one('[data-testid=assistant-error]');
      expect(error.closest('[data-testid=assistant-answer]')).toBeTruthy();
      expect(error.querySelector('pd-glyph')).toBeTruthy();
      expect(error.textContent).toContain('The answer did not finish.');
      expect(text(r, 'assistant-error-cause')).toContain('Likely cause');
      expect(r.one('[data-testid=assistant-answer]').textContent).toContain('Let me explain how the request');
      expect(r.all('[data-testid=assistant-error-details]')).toHaveLength(0);
      r.one('[data-testid=assistant-error-details-toggle]').click();
      await settle(r.fixture);
      expect(text(r, 'assistant-error-details')).toBe('The model stopped answering.');
      r.one('[data-testid=assistant-error-retry]').click();
      for (let i = 0; i < 40; i++) await settle(r.fixture);
      expect(backend.sent.map((s) => s.text)).toEqual(['/fail Explain it', '/fail Explain it']);
      expect(r.all('[data-testid=assistant-turn]')).toHaveLength(2);
      // Only the newest failed turn can be retried.
      expect(r.all('[data-testid=assistant-error-retry]')).toHaveLength(1);
      assertStatusGlyphs(r.one('[data-testid=assistant-error]'));
    });

    it('titles a conversation without its leading slash command, in sentence case (P3-11)', async () => {
      const { r } = await open({
        conversations: [{ ...FIXTURE_CONVERSATION, title: '/slow explain the request pipeline' }],
      });
      r.one('[data-testid=assistant-list-toggle]').click();
      await settle(r.fixture);
      expect(r.one('[data-testid=assistant-open]').textContent?.trim()).toBe('Explain the request pipeline');
      r.one('[data-testid=assistant-open]').click();
      for (let i = 0; i < 4; i++) await settle(r.fixture);
      expect(text(r, 'assistant-title')).toBe('Explain the request pipeline');
      expect(r.one('[data-testid=assistant-title]').getAttribute('title')).toBe('Explain the request pipeline');
    });

    it('dims the context chip together with the inert composer (P3-12)', async () => {
      const pitwall = new MemoryPitwallBackend({ state: { kind: 'notRunning' }, app: 'Demo' });
      const { r } = await open({ pitwall });
      const chip = r.one<HTMLButtonElement>('[data-testid=assistant-chip]');
      expect(chip.disabled).toBe(true);
      expect(chip.classList.contains('inert')).toBe(true);
      pitwall.setState({ kind: 'ready', model: 'q', vision: false, contextTokens: 8192 });
      await settle(r.fixture);
      await settle(r.fixture);
      expect(r.one('[data-testid=assistant-chip]').classList.contains('inert')).toBe(false);
    });

    it('draws the Details disclosure with a 14 px chevron glyph that rotates when open (P3-13)', async () => {
      // A read-only call's details start closed; an action's start open (ADR 0023 D2: its exact arguments are shown).
      const { r } = await open();
      await press(r, await type(r, '/look'), { key: 'Enter' });
      const glyph = r.one('[data-testid=assistant-approval-details] summary pd-glyph');
      expect(glyph.getAttribute('name') ?? glyph.outerHTML).toContain('chevron-right');
      expect(glyph.outerHTML).toMatch(/14/);
      expect(r.element.querySelector('[data-testid=assistant-approval-details]')?.hasAttribute('open')).toBe(false);
    });

    it('moves the focus to Undo after Delete (deviation 1)', async () => {
      const { r } = await open({ conversations: [FIXTURE_CONVERSATION] });
      r.one('[data-testid=assistant-list-toggle]').click();
      await settle(r.fixture);
      vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
      const del = r.one<HTMLButtonElement>('[data-testid=assistant-delete]');
      del.focus();
      del.click();
      await settle(r.fixture);
      await settle(r.fixture);
      expect(document.activeElement).toBe(r.one('[data-testid=assistant-undo-button]'));
    });
  });

  describe('phases and approvals (ADR 0023 WP-4a)', () => {
    it('shows where the answer is before its words: queued, then loading the model with the time so far', async () => {
      const { r, backend } = await open();
      backend.hold();
      await press(r, await type(r, 'q'), { key: 'Enter' });
      backend.emit('m1', { type: 'phase', phase: { kind: 'queued' } });
      await settle(r.fixture);
      expect(text(r, 'assistant-phase')).toBe('Waiting for the model…');
      expect(r.all('[data-testid=assistant-thinking]')).toHaveLength(0);
      backend.emit('m1', { type: 'phase', phase: { kind: 'loading', elapsedMs: 400 } });
      await settle(r.fixture);
      expect(text(r, 'assistant-phase')).toBe('Loading the model…');
      backend.emit('m1', { type: 'phase', phase: { kind: 'loading', elapsedMs: 12_400 } });
      await settle(r.fixture);
      expect(text(r, 'assistant-phase')).toBe('Loading the model… 12 s');
      backend.emit('m1', { type: 'delta', text: 'Hi' });
      await settle(r.fixture);
      expect(r.all('[data-testid=assistant-phase]')).toHaveLength(0);
      backend.release();
    });

    it('offers Always allow when the engine does, sends that choice, and says how each call ran', async () => {
      const { r, backend } = await open();
      await press(r, await type(r, '/look'), { key: 'Enter' });
      await settle(r.fixture);
      const buttons = r.all('[data-testid=assistant-approval] .actions button').map((b) => b.textContent?.trim());
      expect(buttons).toEqual(['Allow once', 'Always allow', 'Decline']);
      expect(r.one<HTMLDetailsElement>('[data-testid=assistant-approval-details]').open).toBe(false);
      r.one<HTMLButtonElement>('[data-testid=assistant-always]').click();
      await settle(r.fixture);
      await settle(r.fixture);
      expect(backend.approvals).toEqual([['call_1', 'alwaysAllow']]);
      expect(text(r, 'assistant-tool-approval')).toBe('Always allowed');
      // The next call of that tool runs without asking, and says so.
      await press(r, await type(r, '/look'), { key: 'Enter' });
      await settle(r.fixture);
      expect(r.all('[data-testid=assistant-approval]')).toHaveLength(0);
      expect(r.all('[data-testid=assistant-tool-approval]').map((e) => e.textContent?.trim())).toEqual(['Always allowed', 'Allowed earlier']);
    });

    it("takes the host app's own words for the empty state and the likely cause of a failure (an app that is the engine)", async () => {
      const own = { emptyHint: 'Ask a local model. Connected apps lend it their tools.', failedCause: 'Likely cause: the model stopped. Try again, or pick another model.' };
      const { r } = await open({ pitwall: null }, own);
      expect(text(r, 'assistant-empty')).toBe(own.emptyHint);
      await press(r, await type(r, '/fail'), { key: 'Enter' });
      await settle(r.fixture);
      expect(text(r, 'assistant-error-cause')).toBe(own.failedCause);
      const plain = await open();
      expect(text(plain.r, 'assistant-empty')).toBe('Ask a question. The assistant can look things up with the tools of this app.');
    });

    it('shows an action with its arguments open and without Always allow; Allow once is said on its card', async () => {
      const { r } = await open();
      r.one<HTMLInputElement>('[data-testid=assistant-allow-actions]').click();
      await press(r, await type(r, '/write'), { key: 'Enter' });
      await settle(r.fixture);
      expect(r.all('[data-testid=assistant-always]')).toHaveLength(0);
      expect(r.one<HTMLDetailsElement>('[data-testid=assistant-approval-details]').open).toBe(true);
      r.one<HTMLButtonElement>('[data-testid=assistant-allow]').click();
      await settle(r.fixture);
      await settle(r.fixture);
      expect(text(r, 'assistant-tool-approval')).toBe('Allowed once');
    });
  });
});
