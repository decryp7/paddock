import type { FieldSpec, SourceKind, WatchAction, WatchDraft } from '../../core/api.types';

/** The form's starting values: every field's default. */
export function defaultValues(kind: SourceKind): Record<string, string> {
  const values: Record<string, string> = {};
  for (const f of kind.fields) if (f.default !== '') values[f.key] = f.default;
  return values;
}

/** A new draft for `kind` (the interval comes from the user's default). */
export function draftFor(base: WatchDraft, kind: SourceKind, intervalSecs: number): WatchDraft {
  return { ...base, id: null, source: { kind: kind.id, values: defaultValues(kind) }, intervalSecs };
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
    kind === 'open-location' ? { kind } : kind === 'download' ? { kind, folder: '', verify: true } : { kind, program: '', args: [] };
  return [...rest, added];
}

/** Replaces one action of its kind. */
export function replaceAction(actions: readonly WatchAction[], action: WatchAction): WatchAction[] {
  return actions.map((a) => (a.kind === action.kind ? action : a));
}

/** Arguments typed one per line; empty lines are dropped. */
export function parseArgs(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
}
