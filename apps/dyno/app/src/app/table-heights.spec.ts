import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';

/**
 * Pages that must keep a fixed table height, by file (relative to `src/`) and the reason. Empty: every Dyno table sizes to the
 * window (`height="fill"` or `height="window"` of pd-table). A chart panel with a fixed canvas would be listed here.
 */
const FIXED_HEIGHT_WAIVERS: Readonly<Record<string, string>> = {};

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.ts$/.test(name) && !/\.spec\.ts$/.test(name) && !/\.d\.ts$/.test(name) ? [path] : [];
  });
}

/** The problems of one source file: a `pd-table` without a chosen height mode, or with a fixed pixel `maxHeight`. */
export function tableHeightProblems(file: string, text: string): string[] {
  if (file in FIXED_HEIGHT_WAIVERS) return [];
  const tags = text.match(/<pd-table\b[^>]*>/g) ?? [];
  return tags.flatMap((tag) => {
    const label = /label="([^"]*)"/.exec(tag)?.[1] ?? '?';
    const problems: string[] = [];
    if (/\[?maxHeight\]?=/.test(tag)) problems.push(`${file}: table "${label}" sets maxHeight; use height="window" or height="fill"`);
    if (!/\sheight="(fill|window)"/.test(tag)) problems.push(`${file}: table "${label}" has no height="fill" or height="window" (Dyno tables size to the window)`);
    return problems;
  });
}

describe('table heights', () => {
  it('no Dyno page gives a table a fixed height: each is "fill" or "window"', () => {
    const root = join(process.cwd(), 'src');
    const files = sources(root);
    expect(files.length).toBeGreaterThan(100);
    expect(files.flatMap((f) => tableHeightProblems(f.slice(root.length + 1), readFileSync(f, 'utf8')))).toEqual([]);
  });

  it('flags a fixed pixel height and a table without a mode', () => {
    expect(tableHeightProblems('a.ts', '<pd-table label="Fixed" [maxHeight]="360" />')).toHaveLength(2);
    expect(tableHeightProblems('a.ts', '<pd-table label="Bare" [rows]="r" />')).toHaveLength(1);
    expect(tableHeightProblems('a.ts', '<pd-table label="Ok" height="window" [rows]="r" />')).toEqual([]);
    expect(tableHeightProblems('a.ts', '<pd-table\n  label="Ok"\n  height="fill"\n>')).toEqual([]);
  });
});
