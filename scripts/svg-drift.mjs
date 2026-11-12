#!/usr/bin/env node
// SVG drift check (ER-40: a generated file is never hand-edited). Re-exports every tracked `.drawio` with draw.io's own CLI,
// the way diagrams/scripts/export-diagrams.sh does, and fails when the committed SVG differs from the export.
//
//   node scripts/svg-drift.mjs [--root <repo>] [--drawio <command>] [--list]
//
// Needs the pinned draw.io desktop (ci-image/Dockerfile: DRAWIO_VERSION; the CI image has it, a workstation can set DRAWIO or
// --drawio). It runs headless: under `xvfb-run -a` when there is no DISPLAY, always with `--no-sandbox`. Exits 0 when nothing
// drifted, 1 on drift, a missing tool or a wrong version (ER-33: a check whose tool is missing never passes), 2 on a bad call.
//
// Comparable practice (ER-37, 2026-11-12): projects that check draw.io exports in CI run the drawio-desktop CLI
// (`--export --format svg`) under xvfb and diff the result (jgraph/drawio-desktop README "Command line usage"; the
// rlespinasse/drawio-export action wraps the same CLI in an image). Same here, with the version pinned and downloaded by SHA-256.
//
// What is compared. An export is not byte-stable across machines: the fallback <text> that draw.io writes beside every HTML
// label (for viewers without foreignObject) is wrapped with the host's font metrics, and a label that overflows its cell moves
// the canvas by a few pixels. So both sides are normalised, then compared:
//   1. the PNG copy of each HTML label is dropped (as export-diagrams.sh does);
//   2. the random id draw.io gives the root of a diagram that uses adaptive colours (`ge-svg-<random>`, new on every export, also
//      in the <style> that names it) is replaced by a fixed one;
//   3. the fallback <text>...</text> after each </foreignObject> inside <switch> is reduced to its words with all white space and
//      tags removed (the font decides where a line wraps, not what it says; its own position and attributes are ignored, the
//      HTML label beside it carries them);
//   4. numbers are compared with a tolerance of FONT_TOLERANCE px when both are at least 8 in magnitude (coordinates, sizes);
//      a smaller number (stroke width, opacity, a dash length) and everything that is not a number (text, colours, ids,
//      element order, attributes) must be equal. `rgb(...)` and `#hex` colours are never numbers.
// So a changed label, colour, shape, connector or a coordinate moved by more than the tolerance is drift; a different font is not.
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/** Pixels two exports of one diagram may differ by when only the host's fonts differ (measured: up to 12 on Pitwall's). */
export const FONT_TOLERANCE = 16;
/** Numbers below this are attributes, not coordinates: equal or drift. */
export const SMALL_NUMBER = 8;

const EXPORT_ARGS = ['--export', '--format', 'svg', '--transparent', '--embed-svg-fonts', 'false', '--border', '16'];

/** Drop what depends on the exporting host: the PNG copies of labels and the fallback text lines. */
export function normalise(svg) {
  return svg
    .replace(/(<\/foreignObject>)<image [^>]*xlink:href="data:image\/png;base64,[^"]*"[^>]*\/>/g, '$1')
    .replace(/(<\/foreignObject>)<text\b[^>]*>(.*?)<\/text>(?=<\/switch>)/gs, (_, fo, inner) => `${fo}<text>${inner.replace(/<[^>]*>/g, '').replace(/\s+/g, '')}</text>`)
    .replace(/ge-svg-[A-Za-z0-9_-]+/g, 'ge-svg-ID');
}

const NUMBER = /rgba?\([^)]*\)|(?<![\w#.])-?\d+(?:\.\d+)?/g;

/** Split into the text between numbers and the numbers themselves (`rgb()` stays text). */
function tokens(s) {
  const nums = [];
  const skeleton = s.replace(NUMBER, (m) => {
    if (m.startsWith('rgb')) return m;
    nums.push(Number(m));
    return '\u0000';
  });
  return { skeleton, nums };
}

/** Whether two already normalised SVGs are the same diagram, up to the font tolerance. Returns null, or why not. */
export function differs(a, b, tolerance = FONT_TOLERANCE) {
  const x = tokens(a);
  const y = tokens(b);
  if (x.skeleton !== y.skeleton || x.nums.length !== y.nums.length) {
    let i = 0;
    while (i < x.skeleton.length && x.skeleton[i] === y.skeleton[i]) i++;
    const show = (t) => t.slice(Math.max(0, i - 40), i + 60).replace(/\u0000/g, '#');
    return `markup differs near "${show(x.skeleton)}" (export) against "${show(y.skeleton)}" (committed)`;
  }
  for (let k = 0; k < x.nums.length; k++) {
    const p = x.nums[k];
    const q = y.nums[k];
    if (p === q) continue;
    const big = Math.abs(p) >= SMALL_NUMBER && Math.abs(q) >= SMALL_NUMBER;
    if (!big || Math.abs(p - q) > tolerance) return `number ${k + 1} is ${p} in the export and ${q} in the committed file`;
  }
  return null;
}

/**
 * Where the committed export of a source lives: `<dir>/svg/<name>.svg` (the layout export-diagrams.sh writes), or, for the two
 * family drawings that docs/images/src/integrations.mjs copies out, `docs/images/<name>.svg`. Null: no committed export is expected.
 */
export function committedSvg(drawio, files) {
  const dir = dirname(drawio);
  const name = basename(drawio, '.drawio');
  const beside = `${dir}/svg/${name}.svg`;
  if (files.has(beside)) return beside;
  if (dir === 'docs/images/src/drawio') return `docs/images/${name}.svg`;
  return null;
}

/** The command that regenerates a source's committed export. */
export function regenerate(drawio, files) {
  const dir = dirname(drawio);
  const name = basename(drawio, '.drawio');
  if (dir === 'docs/images/src/drawio') return 'node docs/images/src/integrations.mjs';
  const app = dir.match(/^(apps\/[^/]+)\/(.*)$/);
  if (app && files.has(`${app[1]}/scripts/export-diagrams.sh`)) {
    return `(cd ${app[1]} && scripts/export-diagrams.sh ${app[2]} ${app[2]} ${name})`;
  }
  return `diagrams/scripts/export-diagrams.sh   (adapt: copy it into the app's scripts/ as the other apps do, then run it for ${name})`;
}

/** The pinned draw.io version: the Dockerfile's ARG, the single source. */
export function pinnedVersion(root) {
  const m = readFileSync(join(root, 'ci-image/Dockerfile'), 'utf8').match(/^ARG DRAWIO_VERSION=(\S+)$/m);
  if (!m) throw new Error('ci-image/Dockerfile has no ARG DRAWIO_VERSION');
  return m[1];
}

function git(root, ...args) {
  const r = spawnSync('git', args, { cwd: root, encoding: 'utf8', maxBuffer: 1 << 28 });
  if (r.status !== 0) throw new Error(`git ${args.join(' ')} failed: ${r.stderr}`);
  return r.stdout;
}

const err = (file, msg) => console.log(`::error${file ? ` file=${file}` : ''}::${msg}`);

/** Run draw.io headless; `viaXvfb` when there is no display. */
function run(drawio, args, viaXvfb) {
  const cmd = viaXvfb ? ['xvfb-run', '-a', drawio, ...args] : [drawio, ...args];
  const started = Date.now();
  const r = spawnSync(cmd[0], cmd.slice(1), { encoding: 'utf8', maxBuffer: 1 << 28 });
  return { ...r, ms: Date.now() - started, cmd: cmd.join(' ') };
}

export function main(argv = process.argv.slice(2), env = process.env) {
  let root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
  let drawio = env.DRAWIO || 'drawio';
  let list = false;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--root') root = resolve(argv[++i] ?? '');
    else if (argv[i] === '--drawio') drawio = argv[++i] ?? '';
    else if (argv[i] === '--list') list = true;
    else {
      err('', `svg-drift: unknown argument '${argv[i]}'. Usage: node scripts/svg-drift.mjs [--root <repo>] [--drawio <command>] [--list]`);
      return 2;
    }
  }

  const files = new Set(git(root, 'ls-files', '-z').split('\0').filter(Boolean));
  const sources = [...files].filter((f) => f.endsWith('.drawio')).sort();
  const checked = [];
  const unchecked = [];
  for (const s of sources) {
    const svg = committedSvg(s, files);
    (svg ? checked : unchecked).push(svg ? { src: s, svg } : s);
  }
  console.log(`svg-drift: ${sources.length} .drawio sources, ${checked.length} with a committed SVG, ${unchecked.length} without one (not checked)`);
  let missing = 0;
  for (const u of unchecked) {
    const dir = dirname(u);
    if ([...files].some((f) => f.startsWith(`${dir}/svg/`) && f.endsWith('.svg'))) {
      missing++;
      err(u, `svg-drift: ${u} has no committed ${dir}/svg/${basename(u, '.drawio')}.svg although its folder keeps exports. Fix: ${regenerate(u, files)}, then commit the SVG.`);
    } else console.log(`  not checked, no committed SVG: ${u}`);
  }
  if (list) return 0;
  if (checked.length === 0) {
    err('', 'svg-drift: no .drawio source has a committed SVG, so the check saw nothing (ER-33). Fix: the layout is <dir>/svg/<name>.svg; see diagrams/README.md.');
    return 1;
  }

  // Precondition (ER-33): the tool exists and is the pinned version.
  const viaXvfb = !env.DISPLAY && !!spawnSync('sh', ['-c', 'command -v xvfb-run'], { encoding: 'utf8' }).stdout.trim();
  const want = pinnedVersion(root);
  const v = run(drawio, ['--version', '--no-sandbox'], viaXvfb);
  // Electron prints D-Bus noise around the version, so take the line that is one.
  const got = `${v.stdout ?? ''}\n${v.stderr ?? ''}`.split('\n').map((l) => l.trim()).findLast((l) => /^\d+\.\d+\.\d+$/.test(l)) ?? '';
  if (v.error || v.status !== 0 || got !== want) {
    err('ci-image/Dockerfile', `svg-drift: draw.io '${drawio}' is ${v.error ? `not runnable (${v.error.code ?? v.error.message})` : `'${got}' (exit ${v.status})`}, the pin is ${want}. Fix: use the CI image built with DRAWIO_VERSION=${want} (ci-image/README.md), or set DRAWIO to that version of draw.io desktop.`);
    return 1;
  }

  const scratch = mkdtempSync(join(env.SVG_DRIFT_TMP || tmpdir(), 'svg-drift-'));
  let drifted = missing;
  let same = 0;
  try {
    const dirs = [...new Set(checked.map((c) => dirname(c.src)))];
    for (const [n, dir] of dirs.entries()) {
      const out = join(scratch, String(n));
      mkdirSync(out, { recursive: true });
      const r = run(drawio, [...EXPORT_ARGS, '--output', out, join(root, dir), '--no-sandbox'], viaXvfb);
      console.log(`svg-drift: exported ${dir} in ${r.ms} ms (exit ${r.status})`);
      if (r.status !== 0) {
        err(dir, `svg-drift: draw.io failed on ${dir} (exit ${r.status}). Command: ${r.cmd}. Tail of stderr: ${(r.stderr ?? '').trim().split('\n').slice(-3).join(' | ')}. Fix: open the diagram in draw.io and export it by hand to see the error.`);
        return 1;
      }
      const made = new Set(readdirSync(out));
      for (const c of checked.filter((x) => dirname(x.src) === dir)) {
        const name = `${basename(c.src, '.drawio')}.svg`;
        if (!made.has(name)) {
          err(c.src, `svg-drift: draw.io wrote no ${name} for ${c.src}. Fix: open it in draw.io; it may be empty or invalid.`);
          drifted++;
          continue;
        }
        const a = normalise(readFileSync(join(out, name), 'utf8'));
        const b = normalise(readFileSync(join(root, c.svg), 'utf8'));
        const why = differs(a, b);
        if (why) {
          drifted++;
          err(c.svg, `${c.svg} differs from a fresh export of ${c.src}: ${why}. The SVG is generated (ER-40), never hand-edited: change the .drawio and regenerate with ${regenerate(c.src, files)}, then commit both.`);
        } else same++;
      }
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
  console.log(`svg-drift: ${same} up to date, ${drifted} drifted (draw.io ${want}, ${viaXvfb ? 'xvfb' : 'display'}, tolerance ${FONT_TOLERANCE} px on fonts)`);
  return drifted ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main());
