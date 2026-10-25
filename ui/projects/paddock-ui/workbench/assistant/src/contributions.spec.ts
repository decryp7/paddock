import { Injector, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { CommandService, ContributionRegistry } from '@decryp7/paddock-ui/shell';
import { assertContributionsValid, provideShellTesting, render, settle } from '@decryp7/paddock-ui/shell/testing';
import { provideWorkbenchContributions, WORKBENCH_PORTS, WorkbenchRegistry, type WorkbenchPorts } from '@decryp7/paddock-ui/workbench';
import { PdAssistant } from './assistant';
import { ASSISTANT_ACTIVITY, ASSISTANT_CONTRIBUTIONS, ASSISTANT_FOCUS_COMMAND, ASSISTANT_VIEW, ASSISTANT_VIEW_ID } from './assistant.contributions';
import { AssistantBackend, MemoryAssistantBackend } from './backend';
import type { ToolCard } from './chat-model';
import { provideAssistant } from './provide-assistant';
import { ASSISTANT_CITATIONS, ASSISTANT_TOOL_RENDERERS, PdAssistantToolCard, provideAssistantRenderers, rendererFor } from './tool-cards';

function ports(shown: string[]): WorkbenchPorts {
  return {
    showView: (id) => {
      shown.push(id);
      return Promise.resolve(true);
    },
    closeView: () => undefined,
    openPanels: signal([]),
    activePanel: signal(null),
    applyPerspective: () => Promise.resolve(true),
    activePerspective: signal(null),
    showExplorer: () => undefined,
    hideExplorer: () => undefined,
    explorerShown: signal(null),
    openPalette: () => undefined,
  };
}

function window_(shown: string[] = [], key?: string | null) {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({
    providers: provideShellTesting({
      providers: [{ provide: WORKBENCH_PORTS, useValue: ports(shown) }, provideWorkbenchContributions(ASSISTANT_CONTRIBUTIONS), provideAssistant({ backend: () => new MemoryAssistantBackend(), ...(key === undefined ? {} : { key }) })],
    }),
  });
}

describe('the assistant contributions', () => {
  it('are valid for the lint, with the view, its activity item and the focus command', () => {
    window_();
    assertContributionsValid(TestBed.inject(Injector));
    const registry = TestBed.inject(WorkbenchRegistry);
    expect(registry.views.map((v) => v.id)).toEqual([ASSISTANT_VIEW_ID]);
    expect(registry.activityItems.map((a) => a.run)).toEqual([{ view: ASSISTANT_VIEW_ID }]);
    expect(TestBed.inject(AssistantBackend)).toBeInstanceOf(MemoryAssistantBackend);
  });

  it('declare a side view of at least 320 px, 400 px by preference', () => {
    expect(ASSISTANT_VIEW).toMatchObject({ id: 'pd.assistant', layout: { area: 'side', min: { width: 320 }, pref: { width: 400 } } });
    expect(ASSISTANT_ACTIVITY.run).toEqual({ view: 'pd.assistant' });
    expect(ASSISTANT_FOCUS_COMMAND).toBe('pd.wb.assistant.focus');
  });

  it('load the pane only when it is shown', async () => {
    const component = await ASSISTANT_VIEW.load();
    expect(component).toBe(PdAssistant);
  });

  it('run the focus command through the workbench ports', async () => {
    const shown: string[] = [];
    window_(shown);
    expect(await TestBed.inject(CommandService).run(ASSISTANT_FOCUS_COMMAND)).toBe(true);
    expect(shown).toEqual(['pd.assistant']);
  });

  it('bind Mod+Shift+A by default and no key when asked', () => {
    const keys = (): string[] => TestBed.inject(ContributionRegistry).items<{ command: string; key: string }>('keybindings').filter((k) => k.command === ASSISTANT_FOCUS_COMMAND).map((k) => k.key);
    window_();
    expect(keys()).toEqual(['Mod+Shift+A']);
    window_([], null);
    expect(keys()).toEqual([]);
    window_([], 'Mod+Alt+A');
    expect(keys()).toEqual(['Mod+Alt+A']);
  });
});

describe('tool renderers', () => {
  it('finds the renderer of a tool and registers the citation handler', () => {
    class A {}
    expect(rendererFor([{ tool: 'a', component: A }], 'a')).toBe(A);
    expect(rendererFor([{ tool: 'a', component: A }], 'b')).toBeNull();
    TestBed.resetTestingModule();
    const handler = { open: () => undefined };
    TestBed.configureTestingModule({ providers: [provideAssistantRenderers({ tools: [{ tool: 'a', component: A }], citations: handler })] });
    expect(TestBed.inject(ASSISTANT_TOOL_RENDERERS)).toHaveLength(1);
    expect(TestBed.inject(ASSISTANT_CITATIONS)).toBe(handler);
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: [provideAssistantRenderers({})] });
    expect(TestBed.inject(ASSISTANT_TOOL_RENDERERS)).toEqual([]);
    expect(TestBed.inject(ASSISTANT_CITATIONS)).toBeNull();
  });

  const card = (over: Partial<ToolCard>): ToolCard => ({ callId: 'c', name: 'find', summary: 'things', write: false, destructive: false, state: 'ok', result: null, data: undefined, ...over });

  it('the default card says what happened in every state', async () => {
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({ providers: provideShellTesting() });
    const r = await render(PdAssistantToolCard, { card: card({ state: 'running' }) });
    const line = (): string => r.one('[data-testid=assistant-tool-result]').textContent ?? '';
    expect(line()).toBe('Working…');
    await r.set({ card: card({ state: 'awaiting' }) });
    expect(line()).toBe('Waiting for your decision');
    await r.set({ card: card({ state: 'ok' }) });
    expect(line()).toBe('Done');
    await r.set({ card: card({ state: 'ok', result: '3 found' }) });
    expect(line()).toBe('3 found');
    await r.set({ card: card({ state: 'failed', name: '' }) });
    expect(line()).toBe('Did not finish');
    expect(r.element.textContent).toContain('(unreadable call)');
    await settle(r.fixture);
  });
});
