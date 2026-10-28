import { ChangeDetectionStrategy, Component, inject, Injectable } from '@angular/core';
import { defineDialog, msg, type DialogDef } from '@decryp7/paddock-ui/shell';
import { DIALOG_REF } from '@decryp7/paddock-ui/shell/dialogs';
import { assertDialogBoots, bootDialog, bootEveryDialog, dialogsOf, dialogTableProblems } from './dialog-harness';
import { ANY_ANSWER, recordIpc } from './ipc-recorder';

/** A service no dialog window provides: injecting it is the blank-window bug. */
@Injectable()
class NotProvided {}

const invoke = (cmd: string) => (globalThis as { __TAURI_INTERNALS__?: { invoke(c: string): Promise<unknown> } }).__TAURI_INTERNALS__?.invoke(cmd);

@Component({
  selector: 'pd-test-ok',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p>Ready</p><button type="button" (click)="ref.close()">Cancel</button><button type="button" class="primary" (click)="ref.close(1)">OK</button>`,
})
class OkContent {
  protected readonly ref = inject(DIALOG_REF);
  constructor() {
    void invoke('app_status');
  }
}

@Component({ selector: 'pd-test-missing', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>never</p>` })
class MissingProviderContent {
  protected readonly s = inject(NotProvided);
}

@Component({ selector: 'pd-test-sneaky', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>Status</p>` })
class UndeclaredCommandContent {
  constructor() {
    void invoke('secret_wipe');
    void invoke('plugin:dialog|open');
  }
}

@Component({ selector: 'pd-test-choice', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>Pick</p><button type="button" class="primary">A</button>` })
class ChoiceContent {}

@Component({ selector: 'pd-test-empty', changeDetection: ChangeDetectionStrategy.OnPush, template: `` })
class EmptyContent {}

@Component({ selector: 'pd-test-logs', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>Logs</p>` })
class LogsContent {
  constructor() {
    console.error('the mock refused /fixtures/a.log');
    console.error(new Error('something else broke'));
    console.error({ circular: 'object' });
  }
}

const dialog = (id: string, kind: DialogDef['kind'], load: () => Promise<unknown>, extra: Partial<DialogDef> = {}): DialogDef =>
  defineDialog({
    id,
    kind,
    modality: kind === 'form' ? 'modal' : undefined,
    title: msg(`t.${id}`, id),
    commands: ['app_status'],
    load: load as DialogDef['load'],
    ...extra,
  }) as DialogDef;

const OK = dialog('t.ok', 'decision', () => Promise.resolve(OkContent), { width: 400 });
const OK_TOOL = dialog('t.tool', 'tool', () => Promise.resolve(OkContent));
const MISSING = dialog('t.missing', 'viewer', () => Promise.resolve(MissingProviderContent));
const SNEAKY = dialog('t.sneaky', 'viewer', () => Promise.resolve(UndeclaredCommandContent));
const CHOICE = dialog('t.choice', 'decision', () => Promise.resolve(ChoiceContent), { requiredChoice: true });
const EMPTY = dialog('t.empty', 'form', () => Promise.resolve(EmptyContent));
const LOGS = dialog('t.logs', 'viewer', () => Promise.resolve(LogsContent));
const ALL = [OK, OK_TOOL, MISSING, SNEAKY, CHOICE, EMPTY, LOGS];

describe('the boot-every-dialog harness', () => {
  it('passes a dialog that boots, sends only declared commands, has a Cancel and closes on Escape', async () => {
    const report = await assertDialogBoots(OK, { dialogs: ALL });
    expect(report.sent).toEqual(['app_status']);
    expect(report.fitted.length).toBeGreaterThan(0);
    const tool = await assertDialogBoots(OK_TOOL, { dialogs: ALL });
    expect(tool.fitted).toEqual([]);
  });

  it('fails on a planted missing provider (the blank window)', async () => {
    const report = await bootDialog(MISSING, { dialogs: ALL });
    expect(report.problems.join('\n')).toMatch(/does not open: .*(NG0201|No provider)/);
    await expect(assertDialogBoots(MISSING, { dialogs: ALL })).rejects.toThrow(/Dialog t\.missing \(viewer\)/);
  });

  it('fails on a planted command the contract does not declare (what Rust would refuse), and ignores plugin commands', async () => {
    const report = await bootDialog(SNEAKY, { dialogs: ALL });
    expect(report.problems).toEqual(['sends secret_wipe, which its contract does not declare']);
    expect(report.sent).toEqual(['secret_wipe']);
    // The app's base set is allowed in every dialog.
    expect((await bootDialog(SNEAKY, { dialogs: ALL, baseCommands: ['secret_*'] })).problems).toEqual([]);
  });

  it('keeps a required choice open on Escape and needs no Cancel', async () => {
    expect((await bootDialog(CHOICE, { dialogs: ALL })).problems).toEqual([]);
  });

  it('fails an empty body', async () => {
    const report = await bootDialog(EMPTY, { dialogs: ALL });
    expect(report.problems).toContain('has an empty body');
  });

  it('reports logged errors, except the expected ones', async () => {
    const report = await bootDialog(LOGS, { dialogs: ALL, ignoreErrors: [/mock refused/] });
    expect(report.problems).toEqual(['logs an error: Error: something else broke', 'logs an error: {"circular":"object"}']);
  });

  it('reports a dialog that is not in the list the window looks it up in, and one whose module fails to load', async () => {
    const report = await bootDialog(OK, { dialogs: [] });
    expect(report.problems.join('\n')).toMatch(/does not open: This dialog is not known/);
    const broken = dialog('t.broken', 'viewer', () => Promise.reject(new Error('chunk missing')));
    const failed = await bootDialog(broken, { dialogs: [broken] });
    expect(failed.problems.join('\n')).toMatch(/does not open: .*chunk missing/);
  });

  it('waits for a dialog module that is still loading, however many turns that takes (no turn budget)', async () => {
    // A first import() of a big screen on a busy runner: pending until the test resolves it by hand.
    let finish!: () => void;
    let asked!: () => void;
    const askedFor = new Promise<void>((r) => (asked = r));
    const module = new Promise<unknown>((resolve) => (finish = () => resolve(OkContent)));
    const slow = dialog('t.slow', 'tool', () => (asked(), module));
    let report: Awaited<ReturnType<typeof bootDialog>> | null = null;
    const booted = bootDialog(slow, { dialogs: [slow] }).then((r) => (report = r));
    await askedFor;
    // Far more macrotask turns than any fixed budget the harness could have given the window: it must still be waiting.
    for (let turn = 0; turn < 450; turn++) await new Promise<void>((r) => setTimeout(r, 0));
    expect(report).toBeNull();
    finish();
    await booted;
    expect(report!.problems).toEqual([]);
  });

  it('reports what it could not check as a problem instead of throwing', async () => {
    const report = await bootDialog(OK, { dialogs: ALL, providers: [{ provide: 'x', useFactory: () => { throw new Error('boom'); } }], inputs: { 't.ok': { a: 1 } } });
    expect(report.problems).toEqual([]);
    const broken = await bootDialog({ ...OK, kind: 'nonsense' } as unknown as DialogDef, { dialogs: ALL });
    expect(broken.problems.some((p) => p.startsWith('the harness could not boot it'))).toBe(true);
  });

  it('boots every dialog of a list and reads the dialogs of registration batches', async () => {
    expect(dialogsOf({ dialogs: [OK] }, {}, { dialogs: [CHOICE] }).map((d) => d.id)).toEqual(['t.ok', 't.choice']);
    const reports = await bootEveryDialog({ dialogs: [OK, CHOICE] });
    expect(reports.map((r) => [r.id, r.problems.length])).toEqual([
      ['t.ok', 0],
      ['t.choice', 0],
    ]);
  });
});

describe('the static command-contract check', () => {
  const table = {
    app_status: { windows: ['main', 'dialog'] },
    app_quit: { windows: ['main'] },
    ui_theme: { windows: ['main', 'popout', 'dialog'] },
    main_only_a: { windows: ['main'] },
  };

  it('accepts commands of the table open to dialogs, prefixes that match one, and plugin commands', () => {
    const d = dialog('t.fine', 'viewer', () => Promise.resolve(OkContent), { commands: ['app_status', 'ui_*', 'plugin:dialog|open'] });
    expect(dialogTableProblems([d], table, ['ui_theme'])).toEqual([]);
  });

  it('names each declared command that is missing, host-only or a prefix that matches nothing usable', () => {
    const d = dialog('t.bad', 'viewer', () => Promise.resolve(OkContent), { commands: ['app_quit', 'nope', 'zz_*', 'main_only_*'] });
    expect(dialogTableProblems([d], table, ['gone'])).toEqual([
      'the base set: declares gone, which is not in the command table',
      't.bad: declares app_quit, which the command table does not let a dialog call (add DIALOG to its entry, reviewed)',
      't.bad: declares nope, which is not in the command table',
      't.bad: declares zz_*, which matches no command of the table',
      't.bad: declares main_only_*, which matches no command open to dialogs',
    ]);
  });
});

describe('the IPC recorder', () => {
  it('records commands, answers from its table, and stops and restores the seam', async () => {
    const host = globalThis as { __TAURI_INTERNALS__?: { invoke(c: string): Promise<unknown>; transformCallback(): number; metadata: unknown } };
    (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ = 'before';
    const rec = recordIpc({ answers: { app_status: { ok: true } } });
    await expect(host.__TAURI_INTERNALS__!.invoke('app_status')).resolves.toEqual({ ok: true });
    expect(await host.__TAURI_INTERNALS__!.invoke('other')).toBe(ANY_ANSWER);
    expect(host.__TAURI_INTERNALS__!.transformCallback()).toBe(1);
    const internals = host.__TAURI_INTERNALS__ as unknown as { unregisterCallback(): void; convertFileSrc(p: string): string };
    internals.unregisterCallback();
    expect(internals.convertFileSrc('/a')).toBe('/a');
    rec.stop();
    let settled = false;
    void host.__TAURI_INTERNALS__!.invoke('late').then(() => (settled = true));
    await Promise.resolve();
    expect(settled).toBe(false);
    expect(rec.sent).toEqual(['app_status', 'other']);
    rec.restore();
    expect((globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__).toBe('before');
    delete (globalThis as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  });

  it('gives an answer that tolerates any use', () => {
    const any = ANY_ANSWER as Record<string | symbol, unknown> & (() => unknown);
    expect(any['deep']).toBe(ANY_ANSWER);
    expect(any()).toBe(ANY_ANSWER);
    expect([...(any as unknown as Iterable<unknown>)]).toEqual([]);
    expect(any['length']).toBe(0);
    expect(any['then']).toBeUndefined();
    expect(`${String(any)}`).toBe('');
  });
});
