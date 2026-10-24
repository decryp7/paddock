#!/usr/bin/env node
// The line-coverage gate over an lcov file: `node scripts/coverage-gate.mjs lcov.info [minimum percent]` (default 99).
//
// One kind of line is not counted: the `#[tauri::command]` attribute itself. Tauri's macro turns the attribute into a
// wrapper (argument extraction with an error arm per argument, the response glue) and llvm-cov attributes every region of
// that wrapper to the attribute's line, so the line reads "not covered" whenever one of its arms cannot be triggered from a
// test (an injected `AppHandle`/`Window` argument never fails to extract, a generic instance for the real runtime is never
// built). The command's body, signature and arguments are separate lines and stay counted. Nothing else is skipped.
import { readFileSync } from 'node:fs';

const GENERATED = /^\s*#\[(::)?tauri::command(\(.*\))?\]\s*$/;

/** Reads lcov text and returns { hit, total, skipped }. `read` gives a source file's text (or null when unreadable). */
export function measure(lcov, read = (p) => { try { return readFileSync(p, 'utf8'); } catch { return null; } }) {
  let file = null;
  let source = null;
  let hit = 0;
  let total = 0;
  let skipped = 0;
  for (const line of lcov.split('\n')) {
    if (line.startsWith('SF:')) {
      file = line.slice(3);
      const text = read(file);
      source = text === null ? null : text.split('\n');
    } else if (line.startsWith('DA:')) {
      const [n, count] = line.slice(3).split(',').map(Number);
      if (source !== null && GENERATED.test(source[n - 1] ?? '')) {
        skipped += 1;
        continue;
      }
      total += 1;
      if (count > 0) hit += 1;
    }
  }
  return { hit, total, skipped };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [path, minimum = '99'] = process.argv.slice(2);
  if (!path) {
    console.error('usage: coverage-gate.mjs <lcov.info> [minimum percent]');
    process.exit(2);
  }
  const { hit, total, skipped } = measure(readFileSync(path, 'utf8'));
  const percent = (100 * hit) / total;
  console.log(`lines ${hit}/${total} = ${percent.toFixed(2)}% (${skipped} generated #[tauri::command] lines not counted)`);
  if (percent < Number(minimum)) process.exit(1);
}
