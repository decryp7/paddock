/*
 * Ranking of the palette's own entries (commands and views; ported from Debrief's `palette-match.ts`, `did-you-mean.ts`).
 * Pure and presentation-only: commands and views are UI contributions. Matching engine data (files, symbols, names) is
 * the engine's work through a palette provider (rule 20), never this.
 */

/** Something the palette lists: an id, a title and an optional category ("View", "Layout"). */
export interface Rankable {
  readonly id: string;
  readonly title: string;
  readonly category?: string;
}

/** A ranked entry: the item, whether its category is shown beside it, and the matched characters of its title. */
export interface Ranked<T extends Rankable> {
  readonly item: T;
  readonly showCategory: boolean;
  /** Character indexes into `item.title` the query matched (for highlighting). */
  readonly marks: ReadonlySet<number>;
}

/** A piece of a title: matched (`mark`) or not. */
export interface TitlePart {
  readonly text: string;
  readonly mark: boolean;
}

const WORD_START = /[\s:._\-/(]/;

function isWordStart(text: string, i: number): boolean {
  return i === 0 || WORD_START.test(text[i - 1] ?? '');
}

function range(start: number, length: number): number[] {
  return Array.from({ length }, (_, i) => start + i);
}

/**
 * Where one query word matches `text`: as the start of a word (best), inside a word (three letters or more), or as the
 * initials of the title's first words ("sa" -> "Show anomalies"). Null otherwise: a scattered subsequence is no match.
 */
export function matchWord(token: string, text: string): { score: number; indices: number[] } | null {
  const t = text.toLowerCase();
  const q = token.toLowerCase();
  if (!q) return { score: 0, indices: [] };
  let at = t.indexOf(q);
  let inside: number | null = null;
  while (at >= 0) {
    if (isWordStart(t, at)) return { score: 10 + q.length * 2 - at / 100, indices: range(at, q.length) };
    inside ??= at;
    at = t.indexOf(q, at + 1);
  }
  if (inside !== null && q.length >= 3) return { score: 4 + q.length - inside / 100, indices: range(inside, q.length) };
  const starts = [...t].map((_, i) => i).filter((i) => isWordStart(t, i) && /\p{L}|\p{N}/u.test(t[i] ?? ''));
  const indices: number[] = [];
  for (const [k, ch] of [...q].entries()) {
    const start = starts[k];
    if (start === undefined || t[start] !== ch) return null;
    indices.push(start);
  }
  return q.length >= 2 ? { score: 3 + q.length, indices } : null;
}

/** Whether the category is shown beside the title: not when the title already starts with it ("AI: Open chat" in AI). */
export function showsCategory(item: Rankable): boolean {
  const category = item.category?.trim().toLowerCase();
  return !!category && !item.title.toLowerCase().startsWith(`${category}:`);
}

/** An entry with nothing matched. */
export function plain<T extends Rankable>(item: T): Ranked<T> {
  return { item, showCategory: showsCategory(item), marks: new Set<number>() };
}

/**
 * Ranks items for the query: every word must match the title (or the full label with its category) by `matchWord`;
 * better and earlier matches first, then items run lately (`recent`) or often reached for (`common`), then shorter titles.
 * An empty query lists `recent`, then `common`, then the rest in their order.
 */
export function rank<T extends Rankable>(items: readonly T[], query: string, recent: readonly string[] = [], common: readonly string[] = []): Ranked<T>[] {
  const tokens = query.trim().split(/\s+/).filter(Boolean);
  if (!tokens.length) {
    const order = (id: string): number => {
      const r = recent.indexOf(id);
      if (r >= 0) return r;
      const c = common.indexOf(id);
      return c >= 0 ? recent.length + c : Number.POSITIVE_INFINITY;
    };
    return items
      .map((item, index) => ({ item, index, rank: order(item.id) }))
      .sort((a, b) => a.rank - b.rank || a.index - b.index)
      .map(({ item }) => plain(item));
  }
  const scored: { entry: Ranked<T>; score: number; index: number }[] = [];
  items.forEach((item, index) => {
    let score = 0;
    const marks = new Set<number>();
    const label = item.category ? `${item.category}: ${item.title}` : item.title;
    for (const token of tokens) {
      const inTitle = matchWord(token, item.title);
      const inLabel = matchWord(token, label);
      if (!inTitle && !inLabel) return;
      if (inTitle && (!inLabel || inTitle.score >= inLabel.score / 2)) {
        score += inTitle.score;
        inTitle.indices.forEach((i) => marks.add(i));
      } else if (inLabel) {
        // A match in the category counts for less: "view" should not rank every View command above "Overview".
        score += inLabel.score / 2;
      }
    }
    const familiar = recent.includes(item.id) ? 2 : common.includes(item.id) ? 1.5 : 0;
    scored.push({ entry: { item, showCategory: showsCategory(item), marks }, score: score + familiar - item.title.length / 1000, index });
  });
  return scored.sort((a, b) => b.score - a.score || a.index - b.index).map((s) => s.entry);
}

/** Splits a title into matched and unmatched runs for rendering. */
export function titleParts(title: string, marks: ReadonlySet<number>): TitlePart[] {
  const parts: { text: string; mark: boolean }[] = [];
  for (let i = 0; i < title.length; i++) {
    const mark = marks.has(i);
    const last = parts.at(-1);
    if (last && last.mark === mark) last.text += title[i];
    else parts.push({ text: title[i] as string, mark });
  }
  return parts;
}

/** The marks of highlight ranges `[start, end)` from a provider, clamped to the label. */
export function marksOf(label: string, highlights: readonly (readonly [number, number])[] | undefined): ReadonlySet<number> {
  const marks = new Set<number>();
  for (const [start, end] of highlights ?? []) {
    const from = Math.max(0, Math.floor(start));
    const to = Math.min(label.length, Math.floor(end));
    for (let i = from; i < to; i++) marks.add(i);
  }
  return marks;
}

/** Optimal-string-alignment distance (insertions, deletions, substitutions, adjacent swaps), case-insensitive. */
export function editDistance(a: string, b: string): number {
  const s = a.toLowerCase();
  const t = b.toLowerCase();
  if (s === t) return 0;
  if (!s.length) return t.length;
  if (!t.length) return s.length;
  let prev2: number[] = [];
  let prev = Array.from({ length: t.length + 1 }, (_, j) => j);
  for (let i = 1; i <= s.length; i++) {
    const cur = [i];
    for (let j = 1; j <= t.length; j++) {
      const cost = s[i - 1] === t[j - 1] ? 0 : 1;
      let v = Math.min((prev[j] ?? 0) + 1, (cur[j - 1] ?? 0) + 1, (prev[j - 1] ?? 0) + cost);
      if (i > 1 && j > 1 && s[i - 1] === t[j - 2] && s[i - 2] === t[j - 1]) v = Math.min(v, (prev2[j - 2] ?? 0) + 1);
      cur.push(v);
    }
    prev2 = prev;
    prev = cur;
  }
  return prev[t.length] ?? 0;
}

/** Typos tolerated in a word of this length: one per three characters, at least one. */
export function tolerance(word: string): number {
  return Math.max(1, Math.floor(word.length / 3));
}

const WORDS = /[\p{L}\p{N}]+/gu;

function words(text: string): string[] {
  return text.toLowerCase().match(WORDS) ?? [];
}

/** How far `query` is from `label` (summed per-word distances; a prefix of a label word is exact), or null when too far. */
export function suggestionScore(query: string, label: string): number | null {
  const q = words(query);
  const l = words(label);
  if (!q.length || !l.length) return null;
  let total = 0;
  for (const w of q) {
    let best = Infinity;
    for (const x of l) best = Math.min(best, x.startsWith(w) ? 0 : Math.min(editDistance(w, x), editDistance(w, x.slice(0, w.length))));
    if (best > tolerance(w)) return null;
    total += best;
  }
  return total;
}

/** "Did you mean": up to `limit` items whose label is closest to `query`, best first (ties keep their order). */
export function didYouMean<T>(items: readonly T[], query: string, label: (item: T) => string, limit = 3): T[] {
  return items
    .map((item, index) => ({ item, index, score: suggestionScore(query, label(item)) }))
    .filter((x): x is { item: T; index: number; score: number } => x.score !== null)
    .sort((a, b) => a.score - b.score || a.index - b.index)
    .slice(0, limit)
    .map((x) => x.item);
}
