import { EnvironmentProviders, Provider, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { recordDialogCommand, type ContributionBatch, type DialogDef } from '@decryp7/paddock-ui/shell';
import {
  allowedCommands,
  commandCovered,
  DIALOG_IO,
  DialogShell,
  dialogWindowConfig,
} from '@decryp7/paddock-ui/shell/dialogs';
import { FakeDialogIo } from './fake-dialog-io';
import { recordIpc } from './ipc-recorder';

/*
 * The boot-every-dialog harness (ADR 0009 §4): every registered dialog boots through the real dialog-window bootstrap
 * (`dialogWindowConfig`: the shell's set, the app's base set and the dialog's own providers) and renders in the real
 * `DialogShell`, with fixture input for the dialogs that need some. It fails on the bug kinds Debrief met one dialog at a
 * time: a missing provider (a blank window), an empty or failed body, a command the contract does not declare, no
 * visible Close/Cancel, Escape that does not dismiss (or dismisses a required choice), and sizes against the kind.
 */

type Providers = readonly (Provider | EnvironmentProviders)[];

export interface DialogHarnessOptions {
  /** Every dialog of the app (the window looks its own up by id, as in the app). */
  readonly dialogs: readonly DialogDef[];
  /**
   * The app's base set of a dialog window (what its `main.ts` passes to `dialogWindowConfig`). A function is called once
   * the IPC recorder is in place, so providers that pick their backend by the Tauri globals take the IPC path.
   */
  readonly providers?: Providers | (() => Providers);
  /** Fixture input per dialog id, for the dialogs that cannot open without any. */
  readonly inputs?: Readonly<Record<string, Readonly<Record<string, unknown>>>>;
  /** The app's commands every dialog window may call (the contract file's `base`). */
  readonly baseCommands?: readonly string[];
  /** Answers of the recorded IPC by command (anything else gets a value that tolerates any use). */
  readonly answers?: Readonly<Record<string, unknown>>;
  /** How many macrotask turns the harness gives a window that never becomes stable before it reports it (default 200). */
  readonly maxTurns?: number;
  /** Console errors that are expected (the mock refusing a fixture path); matched against each message. */
  readonly ignoreErrors?: readonly RegExp[];
}

/** What booting one dialog found. `problems` is empty when the dialog passed every check. */
export interface DialogBootReport {
  readonly id: string;
  readonly kind: DialogDef['kind'];
  readonly problems: readonly string[];
  /** The IPC commands the window sent while it started (plugin commands excluded). */
  readonly sent: readonly string[];
  /** The heights the window asked to be fitted to. */
  readonly fitted: readonly number[];
}

/** The dialogs of an app's registration batches, in order. */
export function dialogsOf(...batches: readonly ContributionBatch<never>[]): DialogDef[] {
  return batches.flatMap((b) => [...(b.dialogs ?? [])] as DialogDef[]);
}

const MISSING_PROVIDER = /NG0\d+|No provider|NullInjector/;

/**
 * Waits for the window's stability (rule 38): the dialog shell's start (its loaders and providers), its debounced checks
 * and any `resource` the content loads are pending tasks, so `whenStable` resolves once the window has settled. Only a
 * window that never settles (a loader that never answers) is given up on after `maxTurns` macrotask turns, so the
 * harness reports it instead of hanging. True when it became stable.
 */
async function settle(fixture: { whenStable(): Promise<unknown> }, maxTurns: number): Promise<boolean> {
  let stable = false;
  void fixture.whenStable().then(() => (stable = true));
  for (let turn = 0; !stable && turn < maxTurns; turn++) await new Promise<void>((r) => setTimeout(r, 0));
  return stable;
}

function describeArgs(args: readonly unknown[]): string {
  return args.map((x) => (x instanceof Error ? `${x.name}: ${x.message}` : typeof x === 'object' ? safeJson(x) : String(x))).join(' ');
}

function safeJson(x: unknown): string {
  try {
    return JSON.stringify(x);
  } catch {
    return String(x);
  }
}

/**
 * Boots dialog `def` as its window would and runs every check. Resets the TestBed before and after; restores the console
 * and the IPC seam. Never throws for a defect of the dialog: each is a sentence in `problems`.
 */
export async function bootDialog(def: DialogDef, options: DialogHarnessOptions): Promise<DialogBootReport> {
  const problems: string[] = [];
  const errors: string[] = [];
  const consoleError = console.error;
  console.error = (...args: unknown[]) => void errors.push(describeArgs(args));
  const ipc = recordIpc({ answers: options.answers });
  const io = new FakeDialogIo();
  io.start = { dialogId: def.id, title: def.title?.text ?? def.id, kind: def.kind, modality: def.modality, requiredChoice: def.requiredChoice === true, input: options.inputs?.[def.id] ?? {} };
  try {
    TestBed.resetTestingModule();
    const providers = typeof options.providers === 'function' ? options.providers() : options.providers;
    const config = dialogWindowConfig({ dialogs: options.dialogs, providers, route: { id: def.id, host: 'main', label: 'd-main-1' }, baseCommands: options.baseCommands });
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...config.providers, { provide: DIALOG_IO, useValue: io }] });
    const fixture = TestBed.createComponent(DialogShell);
    const root = fixture.nativeElement as HTMLElement;
    const loading = () => root.querySelector('[data-testid=dialog-loading]');
    const turns = options.maxTurns ?? 200;
    await settle(fixture, turns);

    const failed = root.querySelector('[data-testid=dialog-error-reason]')?.textContent?.trim();
    if (failed) problems.push(`does not open: ${failed}`);
    else if (loading()) problems.push('never leaves Loading');
    const ignored = options.ignoreErrors ?? [];
    const logged = errors.filter((e) => !ignored.some((r) => r.test(e)));
    for (const e of logged.filter((x) => MISSING_PROVIDER.test(x))) if (!failed || !e.includes(failed)) problems.push(`logs a missing provider: ${e}`);
    if (!failed) for (const e of logged.filter((x) => !MISSING_PROVIDER.test(x))) problems.push(`logs an error: ${e}`);
    const box = root.querySelector('.pd-frame-content');
    const content = box ? [...box.children].filter((c) => !c.matches('[data-testid=dialog-loading], [data-testid=dialog-error], [data-testid=dialog-denied]')) : [];
    if (!failed && !content.some((c) => (c.textContent ?? '').trim() !== '' || c.querySelector('input, button, select, textarea, canvas, svg'))) problems.push('has an empty body');

    // Every command it sent is one its contract (or the base sets) allows: what Rust's guard would refuse fails here. The
    // commands are recorded for the nightly "declared but never sent" check (ADR 0026 §3).
    const allowed = allowedCommands(def, options.baseCommands);
    for (const c of ipc.sent) recordDialogCommand(def.id, c);
    for (const c of [...new Set(ipc.sent)].filter((x) => !commandCovered(allowed, x))) problems.push(`sends ${c}, which its contract does not declare`);

    // The ready handshake (ADR 0026 §4.2): sent once, whatever the kind; Rust sizes the window by the kind.
    if (io.readied === null) problems.push('never sent the ready handshake');
    else if (io.readied.minContent.height > io.readied.content.height || io.readied.minContent.width <= 0) problems.push(`sent a ready measure whose minimum exceeds its content: ${safeJson(io.readied)}`);

    // The frame draws the footer from the declared actions (ADR 0026 §2): a content that draws its own is reported.
    if (box) for (const f of ownFooter(box)) problems.push(`draws its own footer: ${f}`);

    // A visible way out, and Escape per kind: a modal's Cancel, a modeless window's Close; a required choice keeps both.
    const wayOut = root.querySelector('[data-testid=dialog-footer] [data-role=dismiss]');
    if ((def.requiredChoice !== true || failed) && !wayOut) problems.push('has no visible Close/Cancel');
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await settle(fixture, turns);
    const want = def.requiredChoice === true && !failed ? [] : [null];
    if (io.closedWith.length !== want.length || io.closedWith.some((r) => r !== null)) {
      problems.push(want.length ? `does not close on Escape (closed with ${safeJson(io.closedWith)})` : 'is a required choice that Escape closed');
    }
    fixture.destroy();
  } catch (e) {
    problems.push(`the harness could not boot it: ${e instanceof Error ? e.message : String(e)}`);
  } finally {
    ipc.stop();
    TestBed.resetTestingModule();
    ipc.restore();
    console.error = consoleError;
  }
  return { id: def.id, kind: def.kind, problems, sent: [...ipc.sent], fitted: [...io.fitted] };
}

const FOOT_CLASS = /(^|[-_])(foot|footer|actions)$/;
const WAY_OUT = /^(close|cancel|ok)$/i;

/** What a content draws that belongs to the frame's footer: a footer, a foot or actions row, a Close/Cancel/OK button. */
export function ownFooter(content: Element): string[] {
  const out: string[] = [];
  for (const el of content.querySelectorAll('*')) {
    if (el.closest('pd-dialog-frame') !== content.closest('pd-dialog-frame')) continue;
    if (el.tagName === 'FOOTER') out.push('<footer>');
    const token = [...el.classList].find((c) => FOOT_CLASS.test(c));
    if (token) out.push(`.${token}`);
    if (el.tagName === 'BUTTON' && WAY_OUT.test((el.textContent ?? '').replace(/\s+/g, ' ').trim())) out.push(`a "${(el.textContent ?? '').trim()}" button`);
  }
  return out;
}

/** Boots `def` and throws one error listing every problem (for an `it` per dialog). */
export async function assertDialogBoots(def: DialogDef, options: DialogHarnessOptions): Promise<DialogBootReport> {
  const report = await bootDialog(def, options);
  if (report.problems.length) throw new Error(`Dialog ${def.id} (${def.kind}):\n- ${report.problems.join('\n- ')}`);
  return report;
}

/** Boots every dialog of `options.dialogs`, one after another. */
export async function bootEveryDialog(options: DialogHarnessOptions): Promise<DialogBootReport[]> {
  const out: DialogBootReport[] = [];
  for (const def of options.dialogs) out.push(await bootDialog(def, options));
  return out;
}

/** A command table as the generated names module states it (`APP_COMMANDS`): the window classes per command. */
export type CommandTable = Readonly<Record<string, { readonly windows: readonly string[] }>>;

/**
 * The static half of the command contract: every command a dialog (or the base set) declares exists in the app's (or
 * the shell's) command table and is open to dialog windows, and every `prefix*` matches one. Plugin commands are the
 * plugin's. Run it in a spec over the generated `APP_COMMANDS` and `SHELL_COMMANDS`.
 */
export function dialogTableProblems(dialogs: readonly DialogDef[], table: CommandTable, baseCommands: readonly string[] = []): string[] {
  const out: string[] = [];
  const names = Object.keys(table);
  const check = (who: string, c: string) => {
    if (c.startsWith('plugin:')) return;
    if (c.endsWith('*')) {
      const prefix = c.slice(0, -1);
      const hits = names.filter((n) => n.startsWith(prefix));
      if (!hits.length) out.push(`${who}: declares ${c}, which matches no command of the table`);
      else if (!hits.some((n) => table[n]?.windows.includes('dialog'))) out.push(`${who}: declares ${c}, which matches no command open to dialogs`);
      return;
    }
    const entry = table[c];
    if (!entry) out.push(`${who}: declares ${c}, which is not in the command table`);
    else if (!entry.windows.includes('dialog')) out.push(`${who}: declares ${c}, which the command table does not let a dialog call (add DIALOG to its entry, reviewed)`);
  };
  for (const c of baseCommands) check('the base set', c);
  for (const d of dialogs) for (const c of d.commands) check(d.id, c);
  return out;
}
