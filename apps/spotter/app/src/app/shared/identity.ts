/** Pure helpers for a watch's identity tile: a monogram on a hash-coloured square, and the version split used for the dimmed suffix. */

/** One or two capital letters for a name: "Acme Installer" gives "AI", "lumen-cli" gives "LC", "Node.js" gives "N". */
export function monogram(name: string): string {
  const words = name
    .split(/[\s\-_./\\]+/)
    .map((w) => w.replace(/[^\p{L}\p{N}]/gu, ''))
    .filter(Boolean);
  if (!words.length) return '?';
  const first = [...(words[0] ?? '')][0] ?? '?';
  const second = [...(words[1] ?? '')][0] ?? '';
  return (first + second).toUpperCase();
}

/** A stable hue (0..359) for a name; the same name always has the same colour. */
export function hueOf(name: string): number {
  let h = 2166136261;
  for (const ch of name.toLowerCase()) {
    h ^= ch.codePointAt(0) ?? 0;
    h = Math.imul(h, 16777619) >>> 0;
  }
  // Skip the greens next to the accent so an avatar never reads as a status.
  const hue = h % 300;
  return hue + (hue > 95 ? 60 : 0);
}

export interface VersionParts {
  /** The numbers people compare (`R3.10.00`, `2.1.0`). */
  main: string;
  /** The build, channel or pre-release tail (`T32`, `-rc.1`, `+build5`); empty for none. */
  suffix: string;
}

/** Splits a version into its main part and a tail that is shown dimmed. */
export function splitVersion(version: string): VersionParts {
  const m = /^(.*?\d)([-+][^\s]+|[A-Za-z][^\s]*)$/.exec(version);
  return m ? { main: m[1] ?? version, suffix: m[2] ?? '' } : { main: version, suffix: '' };
}

/** Shortens a long identifier (path, hash) in the middle so both ends stay readable. */
export function middleEllipsis(text: string, max: number): string {
  if (text.length <= max || max < 5) return text;
  const keep = max - 1;
  const head = Math.ceil(keep / 2);
  return `${text.slice(0, head)}…${text.slice(text.length - (keep - head))}`;
}
