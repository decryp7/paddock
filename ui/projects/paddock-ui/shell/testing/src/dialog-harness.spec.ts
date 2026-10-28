import { fakeDialogContract } from './fake-contract';
import { ChangeDetectionStrategy, Component, inject, Injectable, signal } from '@angular/core';
import { defineDialog, msg, type DialogDef } from '@decryp7/paddock-ui/shell';
import { DIALOG_REF, type DialogAction } from '@decryp7/paddock-ui/shell/dialogs';
import { assertDialogBoots, bootDialog, bootEveryDialog, dialogsOf, dialogTableProblems } from './dialog-harness';
import { ANY_ANSWER, recordIpc } from './ipc-recorder';

/** A service no dialog window provides: injecting it is the blank-window bug. */
@Injectable()
class NotProvided {}

const invoke = (cmd: string) => (globalThis as { __TAURI_INTERNALS__?: { invoke(c: string): Promise<unknown> } }).__TAURI_INTERNALS__?.invoke(cmd);

@Component({
  selector: 'pd-test-ok',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p>Ready</p>`,
})
class OkContent {
  protected readonly ref = inject(DIALOG_REF);
  readonly dialogActions = signal<readonly DialogAction[]>([{ kind: 'answer', id: 'ok', label: 'OK', role: 'primary', answer: () => 1 }]);
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

@Component({ selector: 'pd-test-choice', changeDetection: ChangeDetectionStrategy.OnPush, template: `<p>Pick</p>` })
class ChoiceContent {
  readonly dialogActions = signal<readonly DialogAction[]>([{ kind: 'answer', id: 'a', label: 'A', role: 'primary', answer: () => 'a' }]);
}

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

const dialog = (id: string, kind: DialogDef['kind'], load: () => Promise<unknown>, extra: { readonly requiredChoice?: boolean; readonly commands?: readonly string[] } = {}): DialogDef =>
  defineDialog(fakeDialogContract({ id, kind, modality: kind === 'form' ? 'modal' : undefined, commands: ['app_status'], ...extra }), { title: msg(`t.${id}`, id), load: load as DialogDef['load'] }) as DialogDef;

const OK = dialog('t.ok', 'decision', () => Promise.resolve(OkContent));
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
    expect(report.fitted).toHaveLength(1);
    const tool = await assertDialogBoots(OK_TOOL, { dialogs: ALL });
    expect(tool.fitted).toHaveLength(1);
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

  it('reports a dialog that is not in the list the window looks it up in, and one whose fixture never loads', async () => {
    const report = await bootDialog(OK, { dialogs: [] });
    expect(report.problems.join('\n')).toMatch(/does not open: This dialog is not known/);
    const stuck = dialog('t.stuck', 'viewer', () => new Promise(() => undefined));
    const never = await bootDialog(stuck, { dialogs: [stuck], maxTurns: 2 });
    expect(never.problems).toContain('never leaves Loading');
  });

  it('reports what it could not check as a problem instead of throwing', async () => {
    const report = await bootDialog(OK, { dialogs: ALL, providers: [{ provide: 'x', useFactory: () => { throw new Error('boom'); } }], inputs: { 't.ok': { a: 1 } } });
    expect(report.problems).toEqual([]);
    const broken = await bootDialog(OK, { dialogs: [null as unknown as DialogDef] });
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
