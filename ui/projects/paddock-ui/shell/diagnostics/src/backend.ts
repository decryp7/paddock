import type { Preview, PreviewReply, SaveOutcome } from '../generated/diagnostics';
import { DIAGNOSTICS_EVENT } from '../generated/diagnostics';
import type { DiagnosticsCommand } from '../generated/diagnostics-commands';
import { parsePreviewReply, parseSaveOutcome } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and
 * an adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface DiagnosticsIpc {
  invoke<T>(command: DiagnosticsCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * Report a problem's port (`paddock_diag::tauri`). The engine collects and redacts; this only shows what it says. The save
 * returns at once (a native dialog asks where, then a job writes the zip); its outcome arrives by `listen`. The webview never
 * supplies a path: only the user's choice in the native dialog is written.
 */
export abstract class DiagnosticsBackend {
  /** Collects the report (without `token`, or with one the engine no longer has) or re-assembles it with the new description. */
  abstract preview(token: string | null, description: string): Promise<PreviewReply>;
  /** Starts the save of the previewed report; rejects with the engine's refusal (`expired`, `busy`, `noPicker`). */
  abstract save(token: string, description: string): Promise<void>;
  /** Cancels the save in progress; false when none runs. */
  abstract cancel(): Promise<boolean>;
  /** Every outcome the engine sends until the returned function is called. */
  abstract listen(handler: (outcome: SaveOutcome) => void): Promise<Unlisten>;
}

/** Report a problem over Tauri. Payloads are parsed at this edge. */
export class TauriDiagnosticsBackend extends DiagnosticsBackend {
  constructor(private readonly ipc: DiagnosticsIpc) {
    super();
  }

  async preview(token: string | null, description: string): Promise<PreviewReply> {
    const reply = parsePreviewReply(await this.ipc.invoke<unknown>('diagnostics_preview', { token, description }));
    if (!reply) throw new Error('The engine sent no valid report preview.');
    return reply;
  }

  async save(token: string, description: string): Promise<void> {
    await this.ipc.invoke<unknown>('diagnostics_save', { token, description });
  }

  async cancel(): Promise<boolean> {
    return (await this.ipc.invoke<unknown>('diagnostics_cancel')) === true;
  }

  listen(handler: (outcome: SaveOutcome) => void): Promise<Unlisten> {
    return this.ipc.listen(DIAGNOSTICS_EVENT, (payload) => {
      const outcome = parseSaveOutcome(payload);
      if (outcome) handler(outcome);
    });
  }
}

/** What the fake engine of [`MemoryDiagnosticsBackend`] does when the user saves. */
export type FakeSave =
  /** The user picks a place; the zip is written. */
  | 'save'
  /** The user closes the save dialog. */
  | 'dismiss'
  /** Writing fails (disk full). */
  | 'fail'
  /** The save waits until it is cancelled (to show the Cancel button). */
  | 'hang';

export interface MemoryDiagnosticsOptions {
  /** The files the fake report holds, in zip order, without the description and manifest (those are added). */
  readonly files: Preview['files'];
  readonly save?: FakeSave;
  /** The path a successful save reports. */
  readonly savedPath?: string;
  /** Runs each simulated step of the engine's work (default: the next microtask). */
  readonly schedule?: (step: () => void) => void;
}

const utf8 = (text: string): number => new TextEncoder().encode(text).length;

/**
 * Report a problem held in memory, playing `paddock-diag` (browser build, demos, tests): the same preview shape, the same
 * outcomes and refusals. The engine is the reference; this only mimics its contract.
 */
export class MemoryDiagnosticsBackend extends DiagnosticsBackend {
  private readonly listeners = new Set<(o: SaveOutcome) => void>();
  private readonly schedule: (step: () => void) => void;
  private issued = 0;
  private token: string | null = null;
  private saving = false;
  private generation = 0;
  /** How often the report was collected (a new description re-assembles it without collecting again). */
  collections = 0;

  constructor(private readonly options: MemoryDiagnosticsOptions) {
    super();
    this.schedule = options.schedule ?? ((step) => queueMicrotask(step));
  }

  private assemble(description: string): Preview {
    const trimmed = description.trim();
    const desc = { name: 'description.txt', kind: 'description' as const, bytes: utf8(trimmed), summary: trimmed ? `Your description (${[...trimmed].length} characters)` : 'No description' };
    const rest = [desc, ...this.options.files];
    const manifest = { name: 'manifest.json', kind: 'manifest' as const, bytes: 400 + rest.length * 90, summary: `The index of the other ${rest.length} files and what was removed from them` };
    const files = [manifest, ...rest];
    return {
      files,
      totalBytes: files.reduce((n, f) => n + f.bytes, 0),
      redactions: { secrets: 2, fields: 1, homePaths: 3, profilePaths: 0, machineNames: 1, userNames: 4, emails: 0, ipAddresses: 1 },
      notes: [],
    };
  }

  preview(token: string | null, description: string): Promise<PreviewReply> {
    if (token === null || token !== this.token) {
      this.collections++;
      this.token = `r${++this.issued}`;
    }
    return Promise.resolve({ token: this.token, preview: this.assemble(description) });
  }

  save(token: string, description: string): Promise<void> {
    if (token !== this.token) {
      return Promise.reject({ code: 'expired', message: 'The preview is out of date. It was made again; look it over and save once more.', retryable: true });
    }
    if (this.saving) return Promise.reject({ code: 'busy', message: 'A report is being saved already.', retryable: true });
    this.saving = true;
    const generation = ++this.generation;
    const total = this.assemble(description);
    const done = (outcome: SaveOutcome): void => {
      this.saving = false;
      for (const l of [...this.listeners]) l(outcome);
    };
    const mode = this.options.save ?? 'save';
    if (mode !== 'hang') {
      this.schedule(() => {
        if (generation !== this.generation) return;
        if (mode === 'dismiss') done({ state: 'dismissed' });
        else if (mode === 'fail') done({ state: 'failed', message: 'Could not write /Volumes/Backup/report.zip: No space left on device. Choose another folder or free some space.' });
        else done({ state: 'saved', path: this.options.savedPath ?? '/Users/you/Desktop/demo-diagnostics-20261024-103000.zip', bytes: Math.round(total.totalBytes / 4), files: total.files.length });
      });
    }
    return Promise.resolve();
  }

  cancel(): Promise<boolean> {
    if (!this.saving) return Promise.resolve(false);
    this.generation++;
    this.schedule(() => {
      this.saving = false;
      for (const l of [...this.listeners]) l({ state: 'cancelled' });
    });
    return Promise.resolve(true);
  }

  listen(handler: (outcome: SaveOutcome) => void): Promise<Unlisten> {
    this.listeners.add(handler);
    return Promise.resolve(() => void this.listeners.delete(handler));
  }
}
