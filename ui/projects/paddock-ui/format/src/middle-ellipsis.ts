/** The ellipsis character that marks removed text. */
export const ELLIPSIS = '…';

/** Longest file name (in characters) kept whole when a path is shortened; a longer name keeps its last characters. */
export const DEFAULT_KEEP_TAIL = 20;

const isSeparator = (c: string | undefined): boolean => c === '/' || c === '\\';

function lastSeparatorIndex(chars: readonly string[]): number {
  for (let i = chars.length - 1; i >= 0; i--) if (isSeparator(chars[i])) return i;
  return -1;
}

const isBoundary = (c: string | undefined): boolean => c === '.' || c === '-' || c === '_' || c === ' ';

/** The first word start at or after `from` (after a `.`, `-`, `_` or space), so the tail never begins mid-word; `from` when there is none. */
function wordStart(chars: readonly string[], from: number): number {
  for (let i = Math.max(1, from); i < chars.length; i++) if (isBoundary(chars[i - 1])) return i;
  return from;
}

/** A text cut into the part that may be shortened and the part that always stays visible. */
export interface SplitText {
  head: string;
  tail: string;
}

/**
 * Splits `text` for a middle ellipsis done by layout (CSS): `tail` is the last separator and file name (or, with no
 * separator or a name longer than `keepTail`, the last `keepTail` characters from the next word start, so a long extension or id stays
 * visible and the tail never begins mid-word)
 * and `head` is the rest, which the layout shortens with an ellipsis. Counts code points, never splitting a surrogate pair.
 */
export function splitForEllipsis(text: string, keepTail = DEFAULT_KEEP_TAIL): SplitText {
  const chars = Array.from(text);
  const keep = Math.max(0, Math.floor(keepTail));
  let start = chars.length - keep;
  const lastSeparator = lastSeparatorIndex(chars);
  // Keep the separator before the file name when the whole name fits: "…\Builder.Service.log".
  if (lastSeparator >= 0 && chars.length - lastSeparator <= keep) start = lastSeparator;
  else if (start > 0) start = wordStart(chars, start);
  if (start <= 0) return { head: '', tail: text };
  return { head: chars.slice(0, start).join(''), tail: chars.slice(start).join('') };
}

/** A path cut into three parts that shrink in order: the directory, then the start of the file name; the tail never shrinks. */
export interface SplitPath {
  /** Everything up to and including the last separator (shrinks first). */
  dir: string;
  /** The start of the file name (shrinks second). */
  name: string;
  /** The end of the file name from a word start, at most `keepTail` characters: the extension and the last word. */
  tail: string;
}

/**
 * Splits `text` for a layout ellipsis that shortens the directory first, then the start of the file name, and keeps the
 * extension and last word whole. A file name of at most `keepTail` characters is all tail. Text with no separator has no `dir`.
 */
export function splitPath(text: string, keepTail = DEFAULT_KEEP_TAIL): SplitPath {
  const chars = Array.from(text);
  const cut = lastSeparatorIndex(chars) + 1;
  const dir = chars.slice(0, cut).join('');
  const { head, tail } = splitForEllipsis(chars.slice(cut).join(''), keepTail);
  return { dir, name: head, tail };
}

/**
 * Shortens `text` to at most `max` characters (code points) by replacing the middle with an ellipsis. A path keeps its
 * start and its file name ("C:\ProgramData\…\Builder.Service.log"); other text keeps its start and its end. `max`
 * below 1 gives an empty string and 1 gives only the ellipsis. Text that fits is returned unchanged.
 */
export function middleEllipsis(text: string, max: number): string {
  const chars = Array.from(text);
  const limit = Math.floor(max);
  if (!(limit >= 1)) return '';
  if (chars.length <= limit) return text;
  if (limit === 1) return ELLIPSIS;
  const budget = limit - 1;
  const lastSeparator = lastSeparatorIndex(chars);
  const nameLength = chars.length - lastSeparator; // with its separator
  // Path: keep the separator and file name when at least two characters of the start fit as well.
  if (lastSeparator > 0 && nameLength <= budget - 2) {
    const headBudget = budget - nameLength;
    let head = chars.slice(0, headBudget);
    // Prefer ending the start on a separator ("C:\ProgramData\") when that keeps at least half of it.
    const cut = lastSeparatorIndex(head);
    if (cut + 1 >= Math.ceil(headBudget / 2)) head = head.slice(0, cut + 1);
    return head.join('') + ELLIPSIS + chars.slice(lastSeparator).join('');
  }
  const tail = Math.floor(budget / 2);
  const head = budget - tail;
  return chars.slice(0, head).join('') + ELLIPSIS + chars.slice(chars.length - tail).join('');
}

/** The leading root of a path that always stays: a UNC share ("\\server\share\"), a drive ("C:\") or a POSIX root ("/"); empty for a relative path. */
const ROOT = /^(?:[\\/]{2}[^\\/]+[\\/][^\\/]+[\\/]?|[A-Za-z]:[\\/]?|[\\/])/;

/** Largest `n` in `[0, max]` for which `fits(n)` holds, given that `fits` is true for small `n` and stays false from some point on. */
function largest(max: number, fits: (n: number) => boolean): number {
  let lo = 0;
  let hi = max;
  while (lo < hi) {
    const mid = Math.ceil((lo + hi) / 2);
    if (fits(mid)) lo = mid;
    else hi = mid - 1;
  }
  return lo;
}

/**
 * Shortens a path to the width `maxWidth` (in whatever unit `measure` returns, normally pixels) with at most one ellipsis and
 * no gap beside it. The root ("C:\", "/", "\\server\share\") is never dropped. What gives way, in order: the folders (one ellipsis
 * for them, flush against the root and the separator: "C:\…\Builder.Service.2026-10-24.log"), then the start of the file name
 * (the ellipsis flush against the root, the end of the name kept with its extension: "C:\…Service.2026-10-24.log"). A path
 * with no root shortens its folders the same way and then its name in the middle (`keepTail`). Text that fits is returned unchanged.
 */
export function fitPath(text: string, maxWidth: number, measure: (s: string) => number, keepTail = DEFAULT_KEEP_TAIL): string {
  if (measure(text) <= maxWidth) return text;
  const chars = Array.from(text);
  const rootLength = Array.from(ROOT.exec(text)?.[0] ?? '').length;
  const cut = Math.max(lastSeparatorIndex(chars) + 1, rootLength); // the directory with its last separator, at least the root
  const root = chars.slice(0, rootLength).join('');
  const name = chars.slice(cut).join('');
  if (cut > rootLength) {
    const sep = chars[cut - 1] ?? '/';
    const middle = chars.slice(rootLength, cut - 1); // the folders between the root and the name
    const build = (n: number): string => root + middle.slice(0, n).join('') + ELLIPSIS + sep + name;
    if (measure(build(0)) <= maxWidth) {
      let n = largest(middle.length, (k) => measure(build(k)) <= maxWidth);
      // End the kept start on a separator when that keeps most of it ("/Users/" rather than "/Users/l").
      const stop = middle.slice(0, n).lastIndexOf(sep);
      if (n < middle.length && stop >= 0 && stop + 1 >= n / 2) n = stop + 1;
      return build(n);
    }
  }
  if (rootLength > 0) {
    const nameChars = Array.from(name);
    const k = largest(nameChars.length, (j) => measure(root + ELLIPSIS + nameChars.slice(nameChars.length - j).join('')) <= maxWidth);
    return root + ELLIPSIS + nameChars.slice(nameChars.length - k).join('');
  }
  return fitName(name === '' ? text : name, maxWidth, measure, keepTail);
}

/** One ellipsis in the middle of a name: its start and its last `keepTail` characters; only the end when that alone is too wide. */
function fitName(name: string, maxWidth: number, measure: (s: string) => number, keepTail: number): string {
  const chars = Array.from(name);
  const { head, tail } = splitForEllipsis(name, keepTail);
  const tailChars = Array.from(tail);
  const headChars = Array.from(head);
  if (headChars.length > 0 && measure(ELLIPSIS + tail) <= maxWidth) {
    const n = largest(headChars.length, (k) => measure(headChars.slice(0, k).join('') + ELLIPSIS + tail) <= maxWidth);
    return headChars.slice(0, n).join('') + ELLIPSIS + tail;
  }
  const k = largest(tailChars.length, (j) => measure(ELLIPSIS + chars.slice(chars.length - j).join('')) <= maxWidth);
  return ELLIPSIS + chars.slice(chars.length - k).join('');
}

/**
 * Shortens ordinary text (a title) to `maxWidth` by cutting its end: the start and one ellipsis, never a space before the
 * ellipsis ("Paddock Sh…", not "Paddock …"). Text that fits is returned unchanged; with no room for a letter, the ellipsis alone.
 */
export function fitEnd(text: string, maxWidth: number, measure: (s: string) => number): string {
  if (measure(text) <= maxWidth) return text;
  const chars = Array.from(text);
  const n = largest(chars.length, (k) => measure(chars.slice(0, k).join('').trimEnd() + ELLIPSIS) <= maxWidth);
  return chars.slice(0, n).join('').trimEnd() + ELLIPSIS;
}
