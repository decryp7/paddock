import { DIALOG_MIN, KIND_MIN_SIZE, KIND_MODALITIES, VIEWER_MIN, type DialogDef, type DialogKind, type DialogModality, type DialogSize } from '@decryp7/paddock-ui/shell';

/*
 * The size and key policy of each dialog kind (ADR 0009 §1), the UI half of `paddock_dialogs::Kind::policy`. The kind
 * decides; a dialog states only what differs. Keep the numbers equal to the Rust constants (`kind.spec.ts` names them).
 */

// The floors are the shell core's (the manifest states them too); re-exported here with the rest of the kind policy.
export { DIALOG_MIN, VIEWER_MIN };
/** A tool's default size (`TOOL_DEFAULT`). */
export const TOOL_DEFAULT: Readonly<DialogSize> = Object.freeze({ width: 960, height: 680 });
/** The width of a decision or form that declares none (`FALLBACK_SIZE.width`). */
export const FALLBACK_WIDTH = 640;

/** What a kind fixes, independent of any one dialog. */
export interface KindPolicy {
  /** The smallest window of the kind (the viewer floor), or none. */
  readonly minSize: Readonly<DialogSize> | null;
  /** The size it opens at when the dialog declares none; a `null` height is fitted to the content. */
  readonly defaultSize: { readonly width: number; readonly height: number | null };
  /** The window is fitted to its content unless the dialog names a height (a viewer always is). */
  readonly fitsContent: boolean;
  /** A re-fit never makes the window shorter (a result being read does not shrink under the reader). */
  readonly growsOnly: boolean;
}

export const KIND_POLICY: Readonly<Record<DialogKind, KindPolicy>> = Object.freeze({
  decision: { minSize: null, defaultSize: { width: FALLBACK_WIDTH, height: null }, fitsContent: true, growsOnly: false },
  form: { minSize: null, defaultSize: { width: FALLBACK_WIDTH, height: null }, fitsContent: true, growsOnly: false },
  viewer: { minSize: KIND_MIN_SIZE.viewer, defaultSize: VIEWER_MIN, fitsContent: true, growsOnly: true },
  tool: { minSize: null, defaultSize: TOOL_DEFAULT, fitsContent: false, growsOnly: false },
});

/** What the shell's own button says and what Escape, Cmd/Ctrl+W and a close request do: a modal's Cancel, else Close. */
export type DialogDismiss = 'cancel' | 'close';

export function dismissOf(modality: DialogModality): DialogDismiss {
  return modality === 'modal' ? 'cancel' : 'close';
}

/** The modality a kind gives a dialog that declared `declared` (`null`: the combination is not allowed). */
export function modalityOf(kind: DialogKind, declared: DialogModality | undefined): DialogModality | null {
  const allowed = KIND_MODALITIES[kind];
  if (!allowed) return null;
  if (declared === undefined) return allowed.length === 1 ? (allowed[0] ?? null) : null;
  return allowed.includes(declared) ? declared : null;
}

/** The size a dialog's window opens at: declared values, then the kind's defaults, never below the kind's floor. */
export interface ResolvedSize {
  readonly width: number;
  /** `undefined`: fitted to the content. */
  readonly height: number | undefined;
  readonly minWidth: number | undefined;
  readonly minHeight: number | undefined;
}

type Sized = Pick<DialogDef, 'kind' | 'width' | 'height' | 'minWidth' | 'minHeight'>;

export function resolveSize(def: Sized): ResolvedSize {
  const p = KIND_POLICY[def.kind];
  const floor = p.minSize;
  const width = Math.max(def.width ?? p.defaultSize.width, floor?.width ?? 0);
  const given = def.height ?? p.defaultSize.height ?? undefined;
  const height = given === undefined ? undefined : Math.max(given, floor?.height ?? 0);
  const minWidth = def.minWidth === undefined && !floor ? undefined : Math.max(def.minWidth ?? 0, floor?.width ?? 0);
  const minHeight = def.minHeight === undefined && !floor ? undefined : Math.max(def.minHeight ?? 0, floor?.height ?? 0);
  return { width, height, minWidth, minHeight };
}

/** Whether the dialog's window is fitted to its content: a viewer always (growing only), a decision or form without a height. */
export function fitsContent(def: Pick<DialogDef, 'kind' | 'height'>): boolean {
  const p = KIND_POLICY[def.kind];
  return p.fitsContent && (p.growsOnly || def.height === undefined);
}

/** A remembered size raised to the kind's floor (a remembered viewer smaller than 680x560 is made 680x560). */
export function raiseToKindMinimum<T extends { width: number; height: number }>(kind: DialogKind, bounds: T): T {
  const floor = KIND_POLICY[kind].minSize;
  return floor ? { ...bounds, width: Math.max(bounds.width, floor.width), height: Math.max(bounds.height, floor.height) } : bounds;
}

/** A short stable key of an input (FNV-1a over its JSON, base 36: letters and digits): the identity of an instanced dialog. */
export function instanceKey(input: unknown): string {
  let h = 0x811c9dc5;
  for (const ch of JSON.stringify(input) ?? '') {
    h ^= ch.charCodeAt(0);
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h.toString(36);
}
