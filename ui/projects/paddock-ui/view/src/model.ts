import type { ActionRef, Icon, Node, NodeKind, Tone } from '../generated/view';

/** The payload of the `NodeKind` variant `K` (`kindPayload<'table'>` is the `Table`). */
export type KindPayload<K extends string> = Extract<NodeKind, Record<K, unknown>>[K];

/** The node kinds this version of the renderer knows (the keys of the externally tagged `NodeKind`). */
export const KNOWN_KINDS = ['stack', 'text', 'table', 'tree', 'graph', 'diagram', 'form', 'reviewList', 'matrix', 'chart', 'badge', 'progress', 'empty', 'needsAi', 'link'] as const;
export type KnownKind = (typeof KNOWN_KINDS)[number];

/** A node seen through its tag: a known kind with its payload, or `unknown` for a kind a newer plugin sent (shown for that node only). */
export type NodeShape = { [K in KnownKind]: { readonly tag: K; readonly v: KindPayload<K> } }[KnownKind] | { readonly tag: 'unknown'; readonly name: string };

/** Reads the externally tagged kind of a node. Never throws: anything that is not a known kind is `unknown`. */
export function shapeOf(node: Node): NodeShape {
  const kind: unknown = node?.kind;
  if (kind === null || typeof kind !== 'object') return { tag: 'unknown', name: '' };
  const name = Object.keys(kind)[0] ?? '';
  const v = (kind as Record<string, unknown>)[name];
  if ((KNOWN_KINDS as readonly string[]).includes(name) && v !== null && typeof v === 'object') return { tag: name, v } as NodeShape;
  return { tag: 'unknown', name };
}

/** Tones the renderer knows; any other value is shown as `neutral`. */
export function toneOf(tone: unknown): Tone {
  return tone === 'info' || tone === 'good' || tone === 'warn' || tone === 'bad' ? tone : 'neutral';
}

/** The glyph (of the `pd:` set) for a tone mark; status is never colour alone, so each tone has its own shape. `neutral` has none. */
export const TONE_GLYPH: Readonly<Record<Tone, string>> = { neutral: '', info: 'info', good: 'check', warn: 'alert', bad: 'close' };

/** The word a screen reader hears for a tone. */
export function toneWord(tone: Tone): string {
  switch (tone) {
    case 'info':
      return $localize`:@@pd.view.tone.info:Note`;
    case 'good':
      return $localize`:@@pd.view.tone.good:Good`;
    case 'warn':
      return $localize`:@@pd.view.tone.warn:Warning`;
    case 'bad':
      return $localize`:@@pd.view.tone.bad:Problem`;
    default:
      return '';
  }
}

/** The glyph of the `pd:` icon set for each icon of the closed `Icon` enum. */
export const ICON_GLYPH: Readonly<Record<Icon, string>> = {
  info: 'info',
  warning: 'alert',
  error: 'alert',
  check: 'check',
  search: 'search',
  folder: 'folder',
  file: 'file',
  table: 'columns',
  graph: 'layers',
  chart: 'activity',
  settings: 'settings',
  ai: 'chat',
  lock: 'lock',
  link: 'external',
};

/** The glyph for an icon; an icon a newer plugin sent shows `info`. */
export function glyphFor(icon: unknown): string {
  return (typeof icon === 'string' && (ICON_GLYPH as Record<string, string>)[icon]) || 'info';
}

/** What the host gets when the user acts: `ViewAction.args` of an action reference with the host's own keys added. */
export function mergeArgs(args: unknown, extra: Readonly<Record<string, unknown>>): unknown {
  if (Object.keys(extra).length === 0) return args ?? null;
  if (args === null || args === undefined) return { ...extra };
  if (typeof args === 'object' && !Array.isArray(args)) return { ...(args as Record<string, unknown>), ...extra };
  // Arguments that are not an object cannot take keys: they travel under `args` beside the host's keys.
  return { args, ...extra };
}

/** Whether an action reference changes something (the UI badges it). */
export const isWrite = (a: ActionRef | null | undefined): boolean => a?.writes === true;

/** A stable key for a child of a stack: its id when it has one, else its place. */
export const childKey = (n: Node, i: number): string => n.id || `#${i}`;
