import { Injector, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { ContributionRegistry, ShellContextHost, SHELL_COMMAND_RUNNER, type ShellCore } from '@decryp7/paddock-ui/shell';
import { contributionProblems } from '@decryp7/paddock-ui/shell/testing';
import { MemoryLogger, provideFakeDock, provideTinyBackendBare } from '../../testing/helpers';
import { contributed } from '../../testing/contributions';
import { appConfig } from '../app.config';
import { Logger } from '../core/logger';
import { WorkspaceStore } from '../core/workspace.store';
import { GridSelection } from '../grid/grid-selection';
import type { RecordView } from '../core/api.types';
import { combineContext, WorkspaceCommandContext } from './command-context';
import { COMMAND_CONTEXT, CommandRegistry, provideDebriefShellCore } from './command-registry';
import { asDebriefCommand, contributeFrom, debriefContributionProviders, DEBRIEF_KIND, keybindingsOf, messageId, toShellCommand } from './contributions';
import type { DebriefCommand, DebriefView } from './tokens';

const record = (id: number): RecordView => ({ id }) as RecordView;

/**
 * Command ids from before the shell's id shape (camelCase parts). Renaming them reaches the native menu (`menu.rs`), the
 * stored key overrides and the specs; M4 deletes `menu.rs`, so the rename is left to it or to a decision before. This
 * list only shrinks: a new camelCase id fails, and so does an entry whose command was renamed (delete the entry).
 */
const CAMEL_CASE_IDS: readonly string[] = [
  'ai.rootCauses', 'analyze.clearFilters', 'analyze.markedOnly', 'analyze.toggleUtc', 'app.restartElevated',
  'app.restartKeepingSession', 'app.restartNormally', 'app.restartSafeMode', 'browser.searchSelected',
  'compare.beforeAfter', 'dashboards.addTile', 'diagnostics.saveDiagnostics', 'dumps.openUmdh', 'dumps.showUmdh',
  'edit.copyJson', 'edit.pasteLog', 'file.openFiles', 'file.openFolder', 'folder.setupAnalysis', 'help.activityLog',
  'help.checkForUpdates', 'help.clearCache', 'help.editShortcuts', 'help.reportProblem', 'help.whatsNew',
  'jobs.clearFinished', 'knownIssues.check', 'notes.addProject', 'notes.addRange', 'notes.addSelected',
  'notifications.markAllRead', 'project.openLast', 'project.saveAs', 'report.addAnnotations', 'report.addAnomalies',
  'report.addFindings', 'report.addNotes', 'report.addQueryTable', 'report.addSelected', 'report.addTimeline',
  'runs.setUp', 'savedQueries.show', 'share.redactedCopy', 'share.redactedView.toggle', 'tools.configDiff.show',
  'view.focusEvents', 'view.focusNextRegion', 'view.focusPreviousRegion', 'view.focusSidebar', 'view.resetLayout',
  'view.sourceColors.instance', 'view.sourceColors.off', 'view.sourceColors.source', 'view.toggleSidebar',
  'visuals.timelinePng', 'visuals.timelineSvg', 'window.autoHide', 'window.autoHideAll', 'window.closeAllDocuments',
  'window.fullScreen', 'window.fullScreenMode.both', 'window.fullScreenMode.chrome', 'window.fullScreenMode.os',
  'window.panelMenu', 'window.pinAll', 'window.popOut',
];


describe('Debrief contributions in the shell registry (M1)', () => {
  it('holds the real app’s commands, keys, views and activity items, and the shell lint finds no defect', () => {
    TestBed.configureTestingModule({ providers: [...appConfig.providers, { provide: Logger, useFactory: () => new MemoryLogger() }] });
    const registry = TestBed.inject(ContributionRegistry);
    expect(registry.commands().length).toBeGreaterThan(50);
    expect(registry.keybindings().length).toBeGreaterThan(10);
    expect(registry.items(DEBRIEF_KIND.views).length).toBeGreaterThan(20);
    expect(registry.items(DEBRIEF_KIND.activityItems).length).toBeGreaterThan(3);
    const problems = contributionProblems(TestBed.inject(Injector));
    const idShape = (id: string) => `command ${id}: id ${JSON.stringify(id)} is not a namespaced id (pw.models.refresh)`;
    expect(problems.filter((p) => !CAMEL_CASE_IDS.some((id) => p === idShape(id)))).toEqual([]);
    expect(CAMEL_CASE_IDS.filter((id) => !problems.includes(idShape(id)))).toEqual([]);
  });

  it('turns a Debrief command into the shell’s CommandDef with live title and category, and its key into a keybinding', () => {
    let title = 'Follow';
    const ran: unknown[] = [];
    const c: DebriefCommand = {
      id: 'live.follow',
      get title() {
        return title;
      },
      category: 'Live',
      keybinding: 'Mod+L',
      view: 'live',
      run: (_ctx, args) => void ran.push(args),
    };
    const def = toShellCommand(c);
    expect(def.title).toMatchObject({ id: 'live.follow', text: 'Follow' });
    expect(def.category).toMatchObject({ id: 'live.follow.category', text: 'Live' });
    title = 'Stop following';
    expect(def.title.text).toBe('Stop following');
    expect(Object.isFrozen(def)).toBe(true);
    expect(keybindingsOf([def])).toEqual([{ command: 'live.follow', key: 'Mod+L', scope: 'window' }]);
    const back = asDebriefCommand(def);
    expect([back.id, back.title, back.category, back.keybinding, back.view]).toEqual(['live.follow', 'Stop following', 'Live', 'Mod+L', 'live']);
    void back.run({} as never, 7);
    expect(ran).toEqual([7]);
  });

  it('spells message ids in the contribution id shape, also for the camelCase command ids', () => {
    expect(messageId('live.follow')).toBe('live.follow');
    expect(messageId('window.pinAll')).toBe('window.pin-all');
    expect(messageId('knownIssues.check', 'category')).toBe('knownissues.check.category');
  });

  it('makes a factory registration’s items on first read of its kind only, so reading views never builds a command class', () => {
    const made: string[] = [];
    const view: DebriefView = { id: 'v', title: 'V', icon: 'chart', component: async () => class {} };
    TestBed.configureTestingModule({
      providers: [
        ...contributeFrom('views', () => (made.push('views'), [view])),
        ...contributeFrom('commands', () => (made.push('commands'), [{ id: 't.x', title: 'X', keybinding: 'F9', run: () => undefined }])),
      ],
    });
    expect(contributed('views').map((v) => v.id)).toEqual(['v']);
    expect(made).toEqual(['views']);
    expect(contributed('commands').map((c) => c.id)).toEqual(['t.x']);
    expect(TestBed.inject(ContributionRegistry).keybindings()).toEqual([{ command: 't.x', key: 'F9', scope: 'window' }]);
    expect(made).toEqual(['views', 'commands']);
  });

  it('gives commands ShellContext & DebriefContext: ctx.runCommand runs another command through Debrief’s registry', async () => {
    const ran: string[] = [];
    TestBed.configureTestingModule({
      providers: [
        provideTinyBackendBare(),
        provideFakeDock(),
        ...debriefContributionProviders({
          commands: [
            { id: 't.outer', title: 'Outer', run: async (ctx) => void ran.push(`outer ${await ctx.runCommand('t.inner')}`) },
            { id: 't.inner', title: 'Inner', run: () => void ran.push('inner') },
          ],
        }),
        provideDebriefShellCore(),
      ],
    });
    expect(await TestBed.inject(CommandRegistry).run('t.outer')).toBe(true);
    expect(ran).toEqual(['inner', 'outer true']);
    expect(await TestBed.inject(SHELL_COMMAND_RUNNER)('t.missing')).toBe(false);
    const ctx = TestBed.inject(COMMAND_CONTEXT);
    expect(typeof ctx.notify).toBe('function');
    ctx.notify({ level: 'error', message: 'Broken' });
    expect(TestBed.inject(WorkspaceStore).error()).toBe('Broken');
  });

  it('reads selection, time range and time display from the window (debrief-1-2-api.md §14.3)', () => {
    TestBed.configureTestingModule({ providers: [provideTinyBackendBare(), provideFakeDock()] });
    const ctx = TestBed.inject(COMMAND_CONTEXT);
    const store = TestBed.inject(WorkspaceStore);
    expect(ctx.workspace).toEqual({ window: 'main' });
    expect(ctx.selection()).toEqual([]);
    store.select(record(4));
    expect(ctx.selection()).toEqual([4]);
    TestBed.inject(GridSelection).publish([record(2), record(3)]);
    expect(ctx.selection()).toEqual([2, 3]);
    expect(ctx.timeRange()).toBeNull();
    store.patchQuery({ from: 10, to: 20 });
    expect(ctx.timeRange()).toEqual({ from: 10, to: 20 });
    store.setTimezone(store.timezone() === 'utc' ? 'local' : 'utc');
    expect(ctx.timeDisplay()).toBe(store.timezone());
  });

  it('lets the shell’s methods win over an app member of the same name, and stays read-only', () => {
    const core = { runCommand: async () => true, showPage: () => undefined, openDialog: async () => ({ kind: 'dismissed' }), notify: () => undefined, quitApp: async () => undefined } as ShellCore;
    const app = { notify: 'app', value: signal(1), get answer() { return this.value(); } };
    const ctx = combineContext(core, app);
    expect(ctx.notify).toBe(core.notify);
    expect(ctx.answer).toBe(1);
    expect(Reflect.set(ctx, 'value', 2)).toBe(false);
    expect(TestBed.inject(ShellContextHost)).toBeDefined();
  });

  it('keeps WorkspaceCommandContext as the Debrief part', () => {
    TestBed.configureTestingModule({ providers: [provideTinyBackendBare(), provideFakeDock()] });
    const show = vi.spyOn(TestBed.inject(WorkspaceCommandContext), 'showView').mockImplementation(() => undefined);
    TestBed.inject(COMMAND_CONTEXT).showView('jobs');
    expect(show).toHaveBeenCalledWith('jobs');
  });
});
