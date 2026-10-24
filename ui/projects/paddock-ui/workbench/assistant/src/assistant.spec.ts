import { ChangeDetectionStrategy, Component, input } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
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

@Component({ selector: 'pd-test-card', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p data-testid="custom-card">{{ card().name }} drawn by the app</p>` })
class CustomCard {
  readonly card = input.required<ToolCard>();
}

interface Options extends MemoryAssistantOptions {
  readonly pitwall?: MemoryPitwallBackend | null;
  readonly handler?: AssistantCitationHandler;
  readonly custom?: boolean;
}

async function open(options: Options = {}) {
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
  const r = await render(PdAssistant);
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
    r.one('[data-testid=assistant-delete]').click();
    for (let i = 0; i < 4; i++) await settle(r.fixture);
    expect(text(r, 'assistant-turn')).toBe('');
    expect(r.one('[data-testid=assistant-list]').textContent).toContain('No conversations yet.');
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
});
