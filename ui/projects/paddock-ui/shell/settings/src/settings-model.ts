import type { SettingsGroup, SettingsPageDef } from '@decryp7/paddock-ui/shell';
import type { FieldKind, SectionView, SettingField, SettingValue } from '../generated/settings';

/*
 * Pure helpers of the Settings window: units and hints for the controls, the convenience pre-check while typing (the
 * engine validates again and its messages count), the search and the navigation. Validation that matters is Rust's
 * (`paddock-settings`); nothing here decides whether a value is stored.
 */

const UNITS_MS: readonly (readonly [number, string])[] = [
  [3_600_000, 'h'],
  [60_000, 'min'],
  [1000, 's'],
];
const UNITS_BYTES: readonly (readonly [number, string])[] = [
  [1 << 30, 'GB'],
  [1 << 20, 'MB'],
  [1 << 10, 'KB'],
];

function largest(n: number, units: readonly (readonly [number, string])[], base: string): string {
  const u = units.find(([size]) => n >= size && n % size === 0);
  return u ? `${n / u[0]} ${u[1]}` : `${n} ${base}`;
}

/** `90000` → `"90 s"` (as `paddock_settings::format_ms`). */
export function formatMs(ms: number): string {
  return largest(ms, UNITS_MS, 'ms');
}

/** `10485760` → `"10 MB"`, binary units (as `paddock_settings::format_bytes`). */
export function formatBytes(bytes: number): string {
  return largest(bytes, UNITS_BYTES, 'bytes');
}

/** The unit a duration or size is edited in: the largest one both limits are whole multiples of. */
export interface EditUnit {
  readonly factor: number;
  readonly label: string;
}

export function editUnit(kind: FieldKind): EditUnit | null {
  const pick = (min: number, max: number, units: readonly (readonly [number, string])[], base: string): EditUnit => {
    const u = units.find(([size]) => min >= size && min % size === 0 && max % size === 0);
    return u ? { factor: u[0], label: u[1] } : { factor: 1, label: base };
  };
  if (kind.type === 'duration') return pick(kind.minMs, kind.maxMs, UNITS_MS, 'ms');
  if (kind.type === 'size') return pick(kind.minBytes, kind.maxBytes, UNITS_BYTES, 'bytes');
  return null;
}

/** The limits shown next to a control ("1–10 times", "1 min – 24 h"). */
export function rangeHint(kind: FieldKind): string {
  const unit = (u: string | null) => (u ? ` ${u}` : '');
  switch (kind.type) {
    case 'int':
    case 'float':
      return `${kind.min}–${kind.max}${unit(kind.unit)}`;
    case 'duration':
      return `${formatMs(kind.minMs)} – ${formatMs(kind.maxMs)}`;
    case 'size':
      return `${formatBytes(kind.minBytes)} – ${formatBytes(kind.maxBytes)}`;
    case 'text':
      return $localize`:@@pd.settings.hint.text:up to ${kind.maxLen}:count: characters`;
    case 'stringList':
      return $localize`:@@pd.settings.hint.list:up to ${kind.maxItems}:count: items, one per line`;
    case 'path':
      return kind.dir ? $localize`:@@pd.settings.hint.folder:folder` : $localize`:@@pd.settings.hint.file:file`;
    default:
      return '';
  }
}

/** A value as the user reads it ("Default: 1 h"). */
export function displayValue(field: SettingField, value: SettingValue): string {
  const k = field.kind;
  if (k.type === 'bool') return value ? 'on' : 'off';
  if (k.type === 'duration' && typeof value === 'number') return formatMs(value);
  if (k.type === 'size' && typeof value === 'number') return formatBytes(value);
  if (k.type === 'enum') return k.options.find((o) => o.value === value)?.label ?? String(value);
  if (Array.isArray(value)) return value.length ? value.join(', ') : 'none';
  if ((k.type === 'int' || k.type === 'float') && k.unit) return `${value} ${k.unit}`;
  return value === '' ? 'not set' : String(value);
}

/** Deep equality of two stored values (they are JSON). */
export function sameValue(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/**
 * Early feedback while typing (types and ranges only). It never decides: the engine validates on apply and its
 * message replaces this one.
 */
export function precheck(field: SettingField, value: unknown): string | null {
  const k = field.kind;
  const num = typeof value === 'number' && Number.isFinite(value) ? value : null;
  switch (k.type) {
    case 'int':
    case 'float':
      if (num === null) return $localize`:@@pd.settings.check.number:Enter a number.`;
      if (k.type === 'int' && !Number.isInteger(num)) return $localize`:@@pd.settings.check.whole:Enter a whole number.`;
      return num < k.min || num > k.max ? `Between ${rangeHint(k)}.` : null;
    case 'duration':
    case 'size': {
      const [min, max] = k.type === 'duration' ? [k.minMs, k.maxMs] : [k.minBytes, k.maxBytes];
      if (num === null) return $localize`:@@pd.settings.check.number:Enter a number.`;
      return num < min || num > max ? `Between ${rangeHint(k)}.` : null;
    }
    case 'text':
      return typeof value === 'string' && [...value].length > k.maxLen ? $localize`:@@pd.settings.check.max-len:At most ${k.maxLen}:count: characters.` : null;
    case 'stringList':
      return Array.isArray(value) && value.length > k.maxItems ? $localize`:@@pd.settings.check.max-items:At most ${k.maxItems}:count: items.` : null;
    default:
      return null;
  }
}

/** What was typed into a number control, in stored units; text that is not a number is passed on for the engine to refuse. */
export function parseNumber(text: string, factor = 1): number | string {
  const trimmed = text.trim();
  if (trimmed === '') return text;
  const n = Number(trimmed);
  return Number.isFinite(n) ? Math.round(n * factor * 1e6) / 1e6 : text;
}

/** One item per line, blank lines dropped. */
export function parseList(text: string): string[] {
  return text
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);
}

/** The words of a text however it is written: case, camelCase, `snake_case`, `kebab-case`, dotted ids (rule 20). */
export function words(text: string): string[] {
  return text
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .toLowerCase()
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
}

/** Whether `a` and `b` differ by at most one edit (insert, delete, change, or two neighbours swapped). */
export function withinOneEdit(a: string, b: string): boolean {
  if (a === b) return true;
  if (Math.abs(a.length - b.length) > 1) return false;
  let i = 0;
  while (i < a.length && i < b.length && a[i] === b[i]) i++;
  const rest = (x: string, n: number) => x.slice(i + n);
  if (a.length === b.length) {
    if (rest(a, 1) === rest(b, 1)) return true;
    return a[i] === b[i + 1] && a[i + 1] === b[i] && rest(a, 2) === rest(b, 2);
  }
  return a.length > b.length ? rest(a, 1) === rest(b, 0) : rest(a, 0) === rest(b, 1);
}

/** Whether some stretch of `text` is within one edit of `word`. */
function nearIn(text: string, word: string): boolean {
  for (let i = 0; i < text.length; i++) {
    for (const len of [word.length - 1, word.length, word.length + 1]) if (withinOneEdit(text.slice(i, i + len), word)) return true;
  }
  return false;
}

/**
 * Search (rule 20): every word of the query is found in the text, however either is written: part of a word, words
 * joined or split (`logfolder`, `log folder`, `logFolder`), and one typo in a word of four letters or more.
 */
export function matchesQuery(text: string, query: string): boolean {
  const asked = words(query);
  if (!asked.length) return false;
  const joined = words(text).join('');
  return asked.every((w) => joined.includes(w) || (w.length >= 4 && nearIn(joined, w)));
}

/** The text a field is found by: its label, help, key, unit and its section's title. */
export function fieldText(section: Pick<SectionView, 'title'>, field: SettingField): string {
  const k = field.kind;
  const unit = k.type === 'int' || k.type === 'float' ? (k.unit ?? '') : '';
  const options = k.type === 'enum' ? k.options.map((o) => o.label).join(' ') : '';
  return `${field.label} ${field.description} ${field.key} ${unit} ${options} ${section.title}`;
}

/** One entry of the Settings navigation: a contributed page, or a section no page shows (listed so nothing is unreachable). */
export interface SettingsNavItem {
  /** The page id (also the Rust section id when there is one). */
  readonly id: string;
  readonly title: string;
  readonly page: SettingsPageDef | null;
  /** The Rust section of the same id, if any. */
  readonly section: SectionView | null;
}

/** Pages of one group that share a sub-group heading (`title` null: the pages without one, listed first). */
export interface SettingsNavSubgroup {
  readonly title: string | null;
  readonly items: readonly SettingsNavItem[];
}

export interface SettingsNavGroup {
  readonly id: SettingsGroup;
  readonly title: string;
  /** Every page of the group in order (the sub-groups flattened). */
  readonly items: readonly SettingsNavItem[];
  /** The same pages under their sub-group headings: unheaded pages first, then each sub-group in order of its first page. */
  readonly subgroups: readonly SettingsNavSubgroup[];
}

/** The group headings, read when shown (so they are in the window's language). */
export const SETTINGS_GROUP_TITLES: Readonly<Record<SettingsGroup, string>> = {
  get application(): string {
    return $localize`:@@pd.settings.group.application:Application`;
  },
  get features(): string {
    return $localize`:@@pd.settings.group.features:Features`;
  },
  get advanced(): string {
    return $localize`:@@pd.settings.group.advanced:Advanced`;
  },
};

/** Splits a group's items under their sub-group headings: unheaded first, then each heading in order of its first page. */
function subgroupsOf(items: readonly SettingsNavItem[]): SettingsNavSubgroup[] {
  const plain: SettingsNavItem[] = items.filter((i) => !i.page?.subgroup);
  const named = new Map<string, SettingsNavItem[]>();
  for (const i of items) {
    const title = i.page?.subgroup?.text;
    if (title) named.set(title, [...(named.get(title) ?? []), i]);
  }
  return [...(plain.length ? [{ title: null, items: plain }] : []), ...[...named].map(([title, list]) => ({ title, items: list }))];
}

/**
 * The navigation: the contributed pages by group and order, then every section no page shows under Advanced (by its
 * title). Empty groups are left out.
 */
export function settingsNav(pages: readonly SettingsPageDef[], sections: readonly SectionView[]): SettingsNavGroup[] {
  const byId = new Map(sections.map((s) => [s.id, s]));
  const shown = new Set(pages.map((p) => p.id));
  const ordered = [...pages].sort((a, b) => a.order - b.order || a.id.localeCompare(b.id));
  const items = (group: SettingsGroup): SettingsNavItem[] => [
    ...ordered.filter((p) => p.group === group).map((p) => ({ id: p.id, title: p.title.text, page: p, section: byId.get(p.id) ?? null })),
    ...(group === 'advanced' ? sections.filter((s) => !shown.has(s.id)).map((s) => ({ id: s.id, title: s.title, page: null, section: s })) : []),
  ];
  return (['application', 'features', 'advanced'] as const)
    .map((id) => {
      const subgroups = subgroupsOf(items(id));
      return { id, title: SETTINGS_GROUP_TITLES[id], items: subgroups.flatMap((s) => s.items), subgroups };
    })
    .filter((g) => g.items.length > 0);
}
