import type { FieldSpec, ProductSetting, SourceKind, Tracking, WatchAction, WatchDraft } from '../../core/api.types';

/** The form's starting values: every field's default. */
export function defaultValues(kind: SourceKind): Record<string, string> {
  const values: Record<string, string> = {};
  for (const f of kind.fields) if (f.default !== '') values[f.key] = f.default;
  return values;
}

/** A new draft for `kind` (the interval comes from the user's default). */
export function draftFor(base: WatchDraft, kind: SourceKind, intervalSecs: number): WatchDraft {
  return { ...base, id: null, source: { kind: kind.id, values: defaultValues(kind) }, schedule: { mode: 'every', secs: intervalSecs } };
}

/** Required fields (secrets excluded: they live in the credential store) that have no value yet. */
export function missingFields(kind: SourceKind | null, values: Record<string, string>): FieldSpec[] {
  if (!kind) return [];
  return kind.fields.filter((f) => f.required && f.kind !== 'secret' && !(values[f.key] ?? '').trim());
}

/** A suggested name for a new watch from its source values. */
export function suggestName(kind: SourceKind | null, values: Record<string, string>): string {
  if (!kind) return '';
  const first = kind.fields.find((f) => f.kind !== 'secret' && f.kind !== 'toggle' && (values[f.key] ?? '').trim());
  const raw = first ? (values[first.key] ?? '').trim() : '';
  const tail = raw.replace(/[?#].*$/, '').split(/[\\/]/).filter(Boolean).pop() ?? '';
  return tail ? tail : kind.label;
}

export type ActionKind = WatchAction['kind'];

export function findAction<K extends ActionKind>(actions: readonly WatchAction[], kind: K): Extract<WatchAction, { kind: K }> | null {
  return (actions.find((a) => a.kind === kind) as Extract<WatchAction, { kind: K }> | undefined) ?? null;
}

/** Turns an action on (with its defaults) or off, keeping the others. */
export function toggleAction(actions: readonly WatchAction[], kind: ActionKind, on: boolean): WatchAction[] {
  const rest = actions.filter((a) => a.kind !== kind);
  if (!on) return rest;
  const added: WatchAction =
    kind === 'open-location' ? { kind, variant: '', extension: '' } : kind === 'download' ? { kind, folder: '', verify: true, variant: '', extension: '' } : { kind, program: '', args: [] };
  return [...rest, added];
}

/** Replaces one action of its kind. */
export function replaceAction(actions: readonly WatchAction[], action: WatchAction): WatchAction[] {
  return actions.map((a) => (a.kind === action.kind ? action : a));
}

/** The file types offered as chips. */
export const EXTENSION_CHIPS = ['iso', 'exe', 'msi', 'zip', 'dmg', 'pkg'] as const;

/** The extensions in a comma separated list, as typed. */
export function extensionList(value: string): string[] {
  return value
    .split(/[,;\s]+/)
    .map((e) => e.trim().replace(/^[.*]+/, '').toLowerCase())
    .filter(Boolean);
}

/** Adds or removes one extension in the list, keeping the rest (including custom ones) in order. */
export function toggleExtension(value: string, ext: string, on: boolean): string {
  const list = extensionList(value).filter((e) => e !== ext);
  return (on ? [...list, ext] : list).join(', ');
}

/** The product list after ticking or unticking `id`: an empty list means every product, so unticking one narrows it. */
export function toggleTracked(only: readonly string[], discovered: readonly string[], id: string, on: boolean): string[] {
  const current = only.length ? [...only] : [...discovered];
  const next = on ? [...new Set([...current, id])] : current.filter((p) => p !== id);
  return discovered.length > 0 && discovered.every((p) => next.includes(p)) && next.every((p) => discovered.includes(p)) ? [] : next;
}

/** One product's settings changed; settings that are all defaults are dropped. */
export function patchProduct(tracking: Tracking, id: string, patch: Partial<ProductSetting>): Tracking {
  const next: ProductSetting = { name: '', muted: false, soakHours: null, ...tracking.products.settings[id], ...patch };
  const settings = { ...tracking.products.settings };
  if (next.name.trim() === '' && !next.muted && next.soakHours === null) delete settings[id];
  else settings[id] = { ...next, name: next.name };
  return { ...tracking, products: { ...tracking.products, settings } };
}

/** Arguments typed one per line; empty lines are dropped. */
export function parseArgs(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
}
