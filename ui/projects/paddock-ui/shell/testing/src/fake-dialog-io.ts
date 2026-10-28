import { DIALOG_EVENTS, type DialogInfo, type DialogRemembered, type DialogRequest, type DialogStart } from '@decryp7/paddock-ui/shell';
import type { DialogIo, DialogMeasure, Unlisten } from '@decryp7/paddock-ui/shell/dialogs';

/** A scripted `DialogIo` for specs: records every call and plays the dialog layer's events. */
export class FakeDialogIo implements DialogIo {
  readonly opened: DialogRequest[] = [];
  readonly closeRequests: string[] = [];
  /** The results the dialog closed with, in order (`undefined`: dismissed). */
  readonly closedWith: unknown[] = [];
  /** The titles the dialog set at run time, in order. */
  readonly titles: string[] = [];
  /** The heights the dialog asked its window to fit (the ready handshake's first), in order. */
  readonly fitted: number[] = [];
  /** The ready handshake's measure (`null` until the frame sent it). */
  readied: DialogMeasure | null = null;
  /** Every measure sent (ready, then each fit), with its minimum content. */
  readonly measures: DialogMeasure[] = [];
  /**
   * When true, `ready` never settles: the window system has not answered, as when the webview is paused, so a spec can
   * drive the show deadline (ADR 0026 §4.3).
   */
  withholdReady = false;
  private readonly handlers = new Map<string, Set<(p: unknown) => void>>();
  private readonly labels = new Map<string, string>();
  private next = 1;
  /** When set, `open` fails with it (the window system refused). */
  failOpen: Error | null = null;
  /** What `take` answers. */
  start: DialogStart = { dialogId: 'x', title: 'X', kind: 'form', modality: 'modeless', requiredChoice: false, input: {} };

  async open(request: DialogRequest): Promise<DialogInfo> {
    this.opened.push(request);
    if (this.failOpen) throw this.failOpen;
    const key = request.instance ? `${request.dialogId}#${request.instance}` : request.dialogId;
    const known = this.labels.get(key);
    const label = known ?? `d-main-${this.next++}`;
    this.labels.set(key, label);
    return { label, dialogId: request.dialogId, existing: known !== undefined };
  }
  async take(): Promise<DialogStart> {
    return this.start;
  }
  async close(result: unknown): Promise<void> {
    this.closedWith.push(result);
  }
  async listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten> {
    const set = this.handlers.get(event) ?? new Set();
    set.add(handler);
    this.handlers.set(event, set);
    return () => set.delete(handler);
  }
  async requestClose(label: string): Promise<void> {
    this.closeRequests.push(label);
  }
  async setTitle(title: string): Promise<void> {
    this.titles.push(title);
  }
  ready(measure: DialogMeasure): Promise<void> {
    this.readied = measure;
    this.measures.push(measure);
    this.fitted.push(measure.content.height);
    return this.withholdReady ? new Promise<void>(() => undefined) : Promise.resolve();
  }
  async fit(measure: DialogMeasure): Promise<void> {
    this.measures.push(measure);
    this.fitted.push(measure.content.height);
  }

  /** Plays an event of the dialog layer. */
  emit(event: string, payload: unknown): void {
    for (const h of this.handlers.get(event) ?? []) h(payload);
  }
  /** The dialog `label` answered `result`. */
  result(label: string, dialogId: string, result: unknown): void {
    this.emit(DIALOG_EVENTS.result, { label, dialogId, result });
  }
  /** The dialog window `label` is gone (and forgets its label, so opening it again opens a new one). */
  closed(label: string, dialogId: string, remembered: DialogRemembered | null = null): void {
    for (const [k, v] of this.labels) if (v === label) this.labels.delete(k);
    this.emit(DIALOG_EVENTS.closed, { label, dialogId, remembered });
  }
  /** A close request reached this dialog window (the OS close, or the opener's `close`). */
  closeRequested(): void {
    this.emit(DIALOG_EVENTS.closeRequested, null);
  }
  listeners(event: string): number {
    return this.handlers.get(event)?.size ?? 0;
  }
}
