import type { GroupSpan } from '../../core/api.types';

/** A run of a name: plain text, or text one group of the pattern matched. */
export interface Segment {
  text: string;
  /** The group that matched this run (`id`, `major`, ...), or null for plain text. */
  group: string | null;
}

/**
 * Cuts a name into runs by where the engine says each group matched (spans are in UTF-16 code units, which is what
 * JavaScript strings index by). Where groups nest, the innermost (shortest) one colours the text. Pure layout; the
 * matching itself is done in Rust.
 */
export function segments(text: string, spans: readonly GroupSpan[]): Segment[] {
  const owner: (GroupSpan | null)[] = Array.from({ length: text.length }, () => null);
  for (const s of spans) {
    const start = Math.max(0, s.start);
    const end = Math.min(text.length, s.end);
    for (let i = start; i < end; i++) {
      const cur = owner[i];
      if (!cur || s.end - s.start < cur.end - cur.start) owner[i] = s;
    }
  }
  const out: Segment[] = [];
  for (let i = 0; i < text.length; i++) {
    const group = owner[i]?.group ?? null;
    const last = out[out.length - 1];
    if (last && last.group === group) last.text += text[i];
    else out.push({ text: text[i]!, group });
  }
  return out;
}

/** How a group is named in the legend and in the title of a highlighted run. */
export function groupLabel(group: string): string {
  switch (group) {
    case 'id':
      return 'product id';
    case 'major':
    case 'minor':
    case 'patch':
    case 'build':
      return `${group} number`;
    case 'channel':
    case 'pre':
      return 'channel';
    case 'variant':
      return 'variant';
    default:
      return `field "${group}"`;
  }
}

/** The CSS class of a group (numbers share one family, the rest have their own). */
export function groupClass(group: string): string {
  if (group === 'id') return 'g-id';
  if (group === 'major' || group === 'minor' || group === 'patch' || group === 'build') return 'g-num';
  if (group === 'channel' || group === 'pre') return 'g-channel';
  if (group === 'variant') return 'g-variant';
  return 'g-field';
}
