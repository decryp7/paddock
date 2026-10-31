import { reflectComponentType, type ComponentRef } from '@angular/core';

/** What re-targeting an open dialog carries: its id and the new inputs (plain data). */
interface Retarget {
  /** The page that sent it, so a page ignores its own echo (it already told its own dialogs directly). */
  readonly from: string;
  readonly dialogId: string;
  readonly inputs: Readonly<Record<string, unknown>>;
}

const CHANNEL = 'paddock-dialog-inputs';
const PAGE = typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function' ? crypto.randomUUID() : `p${Math.random().toString(36).slice(2)}`;
type Listener = (inputs: Readonly<Record<string, unknown>>) => void;
const local = new Map<string, Set<Listener>>();
let outgoing: BroadcastChannel | null | undefined;

function parse(data: unknown): Retarget | null {
  if (typeof data !== 'object' || data === null) return null;
  const { from, dialogId, inputs } = data as Record<string, unknown>;
  return typeof from === 'string' && typeof dialogId === 'string' && typeof inputs === 'object' && inputs !== null && !Array.isArray(inputs)
    ? { from, dialogId, inputs: inputs as Record<string, unknown> }
    : null;
}

function channel(): BroadcastChannel | null {
  if (outgoing === undefined) outgoing = typeof BroadcastChannel === 'function' ? new BroadcastChannel(CHANNEL) : null;
  return outgoing;
}

/**
 * Re-targets the open dialog `dialogId`: its content gets `inputs` (each declared input is set again). A dialog in this page
 * hears it directly; a dialog in another window of the same app hears it on a same-origin `BroadcastChannel`, so any dialog can
 * be pointed somewhere new (Settings at a section) when it is already open instead of only being focused.
 */
export function sendDialogInputs(dialogId: string, inputs: Readonly<Record<string, unknown>>): void {
  for (const l of local.get(dialogId) ?? []) l(inputs);
  try {
    channel()?.postMessage({ from: PAGE, dialogId, inputs } satisfies Retarget);
  } catch {
    // Not cloneable or no channel: only this page's dialogs hear it.
  }
}

/** Applies later inputs for `dialogId` to `ref` (the inputs its component declares); returns what stops listening. */
export function followDialogInputs(dialogId: string, ref: ComponentRef<unknown>): () => void {
  const declared = new Set((reflectComponentType(ref.componentType)?.inputs ?? []).map((i) => i.templateName));
  const apply: Listener = (inputs) => {
    for (const [name, value] of Object.entries(inputs)) if (declared.has(name)) ref.setInput(name, value);
    ref.changeDetectorRef.detectChanges();
  };
  let set = local.get(dialogId);
  if (!set) local.set(dialogId, (set = new Set()));
  set.add(apply);
  let incoming: BroadcastChannel | null = null;
  if (typeof BroadcastChannel === 'function') {
    incoming = new BroadcastChannel(CHANNEL);
    incoming.onmessage = (e: MessageEvent<unknown>) => {
      const m = parse(e.data);
      if (m && m.from !== PAGE && m.dialogId === dialogId) apply(m.inputs);
    };
  }
  return () => {
    set.delete(apply);
    incoming?.close();
  };
}
