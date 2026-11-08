import { DIALOG_KIND_TABLE, DIALOG_MIN, KIND_MODALITIES, VIEWER_MIN, type DialogDef, type DialogKind, type DialogModality, type DialogSize } from '@decryp7/paddock-ui/shell';

/*
 * The size and key policy of each dialog kind (ADR 0009 §1, ADR 0026 §1): only functions over the generated kind table
 * (`DIALOG_KIND_TABLE`, from `paddock_dialogs::Kind::policy`). The numbers are never written here, so TypeScript and
 * Rust cannot disagree (ADR 0016 cause 6).
 */

export { DIALOG_MIN, VIEWER_MIN };
/** A tool's default size (`TOOL_DEFAULT`). */
export const TOOL_DEFAULT: Readonly<DialogSize> = Object.freeze({ width: DIALOG_KIND_TABLE.tool.width, height: DIALOG_KIND_TABLE.tool.height });

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

/** The size a dialog opens at: its contract's width and preferred height (`undefined`: measured), never below its kind's floor. */
export interface ResolvedSize {
  readonly width: number;
  readonly height: number | undefined;
  readonly minWidth: number;
  readonly minHeight: number;
}

export function resolveSize(def: Pick<DialogDef, 'kind' | 'width' | 'height'>): ResolvedSize {
  const floor = DIALOG_KIND_TABLE[def.kind].minSize ?? DIALOG_MIN;
  return {
    width: Math.max(def.width, floor.width),
    height: def.height === null ? undefined : Math.max(def.height, floor.height),
    minWidth: floor.width,
    minHeight: floor.height,
  };
}

/** How the dialog's window follows its content (ADR 0043 §1): the kind table's `sizeMode`, `height` for decisions, forms and viewers, `manual` for a tool. */
export function sizeModeOf(def: Pick<DialogDef, 'kind'>): 'height' | 'manual' {
  return DIALOG_KIND_TABLE[def.kind].sizeMode;
}

/** Whether a `height` window never shrinks below its size (a viewer: a result being read does not shrink under the reader). */
export function growsOnly(def: Pick<DialogDef, 'kind'>): boolean {
  return DIALOG_KIND_TABLE[def.kind].shrink === 'never';
}

/** A remembered size raised to the kind's floor (a remembered viewer smaller than 680x560 is made 680x560). */
export function raiseToKindMinimum<T extends { width: number; height: number }>(kind: DialogKind, bounds: T): T {
  const floor = DIALOG_KIND_TABLE[kind].minSize;
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
