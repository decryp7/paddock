#!/usr/bin/env node
// SVG drift check (ER-40: a generated file is never hand-edited). Re-exports every tracked `.drawio` with draw.io's own CLI,
// the way diagrams/scripts/export-diagrams.sh does, and fails when the committed SVG differs from the export.
//
//   node scripts/svg-drift.mjs [--root <repo>] [--drawio <command>] [--list] [--keep <dir>]   (--keep: leave the exports in <dir>/<source folder>)
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
//   4. numbers are compared exactly (font sizes, widths, heights, every y coordinate, stroke widths, path control points),
//      with two measured exceptions, both caused by a label that overflows its box in the host's font (docs/benchmarks.md,
//      "SVG drift check: font metrics"): the root `width`, `height` and `viewBox` of the <svg> tag may differ by up to
//      ROOT_TOLERANCE px, and the whole drawing may be translated sideways, that is, EVERY x coordinate (`x`, `cx`, `x1`,
//      `x2`, `margin-left`, the x of each path command) differs by the same amount, at most ROOT_TOLERANCE px, and every
//      other number is equal. `rgb(...)` and `#hex` colours are never numbers.
// So a changed label, colour, shape, connector, font size, size, or one box moved is drift; a different font is not.
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { basename, dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

/**
 * The most the root width/height/viewBox, and the sideways translation of a whole drawing, may differ between two exports of one
 * diagram that differ only in the host's fonts. Measured 2026-11-12 over 143 committed SVGs against draw.io 32.6.10 on Linux amd64:
 * 132 identical, 10 differ only that way with a maximum of 12 (Pitwall uml-use-cases-clients; docs/benchmarks.md).
 */
export const ROOT_TOLERANCE = 12;

const EXPORT_ARGS = ['--export', '--format', 'svg', '--transparent', '--embed-svg-fonts', 'false', '--border', '16'];

/** Drop what depends on the exporting host: the PNG copies of labels and the fallback text lines. */
export function normalise(svg) {
  return svg
    .replace(/(<\/foreignObject>)<image [^>]*xlink:href="data:image\/png;base64,[^"]*"[^>]*\/>/g, '$1')
    .replace(/(<\/foreignObject>)<text\b[^>]*>(.*?)<\/text>(?=<\/switch>)/gs, (_, fo, inner) => `${fo}<text>${inner.replace(/<[^>]*>/g, '').replace(/\s+/g, '')}</text>`)
    .replace(/ge-svg-[A-Za-z0-9_-]+/g, 'ge-svg-ID');
}

const NUMBER = /rgba?\([^)]*\)|(?<![\w#.])-?\d+(?:\.\d+)?/g;

/** Split into the skeleton (numbers replaced by NUL; `rgb()` stays text) and each number with its place in the string. */
function tokens(s) {
  const nums = [];
  const skeleton = s.replace(NUMBER, (m, at) => {
    if (m.startsWith('rgb')) return m;
    nums.push({ v: Number(m), at, end: at + m.length });
    return '\u0000';
  });
  return { skeleton, nums };
}

/** Whether the number at `at` is an x coordinate: an x-like attribute, `margin-left`, or the x of a path command. */
function isX(s, at, end) {
  if (s[end] === '%') return false; // x="50%" is a position in the viewport, not a coordinate
  const before = s.slice(0, at);
  if (/(?:^|[\s"<])(?:x|cx|x1|x2)="$/.test(before) || /margin-left: $/.test(before)) return true;
  const d = before.lastIndexOf(' d="');
  if (d < 0 || before.indexOf('"', d + 4) >= 0) return false;
  const cmd = before.slice(d + 4).match(/([A-Za-z])([^A-Za-z]*)$/);
  if (!cmd) return false;
  const index = cmd[2].split(/[\s,]+/).filter(Boolean).length; // numbers of this command before ours
  switch (cmd[1].toUpperCase()) {
    case 'H': return true;
    case 'V': case 'Z': return false;
    case 'A': return index % 7 === 5;
    default: return index % 2 === 0; // M L T Q C S: x, y pairs
  }
}

/** Whether two already normalised SVGs are the same diagram, up to the host's fonts (see the header). Returns null, or why not. */
export function differs(a, b, tolerance = ROOT_TOLERANCE) {
  const x = tokens(a);
  const y = tokens(b);
  if (x.skeleton !== y.skeleton || x.nums.length !== y.nums.length) {
    let i = 0;
    while (i < x.skeleton.length && x.skeleton[i] === y.skeleton[i]) i++;
    const show = (t) => t.slice(Math.max(0, i - 40), i + 60).replace(/\u0000/g, '#');
    return `markup differs near "${show(x.skeleton)}" (export) against "${show(y.skeleton)}" (committed)`;
  }
  const rootEnd = a.indexOf('>', Math.max(0, a.indexOf('<svg'))); // the end of the <svg ...> tag
  const xs = [];
  for (let k = 0; k < x.nums.length; k++) {
    const p = x.nums[k].v;
    const q = y.nums[k].v;
    if (x.nums[k].at > 0 && x.nums[k].at < rootEnd && x.nums[k].at > a.indexOf('<svg')) {
      if (Math.abs(p - q) > tolerance) return `the root size is ${p} in the export and ${q} in the committed file, more than ${tolerance} px apart`;
    } else if (isX(a, x.nums[k].at, x.nums[k].end)) xs.push({ k, d: p - q });
    else if (p !== q) return `number ${k + 1} (not an x coordinate) is ${p} in the export and ${q} in the committed file`;
  }
  const t = xs.find(({ d }) => d !== 0)?.d ?? 0;
  if (Math.abs(t) > tolerance) return `x coordinates moved by ${t} px, more than ${tolerance}`;
  const odd = xs.find(({ d }) => d !== t);
  if (odd) return `x coordinate ${odd.k + 1} (after "${a.slice(Math.max(0, x.nums[odd.k].at - 30), x.nums[odd.k].at).replace(/\s+/g, ' ')}") is ${x.nums[odd.k].v} in the export and ${y.nums[odd.k].v} in the committed file, but the others differ by ${t} px: one box moved, not the host's fonts`;
  return null;
}

/**
 * Where each source folder's committed exports live, when that is not `<folder>/svg` (the layout export-diagrams.sh writes
 * under its out-dir). The one declared table: add a line here for a folder that keeps its SVGs elsewhere.
 */
export const EXPORT_DIRS = {
  'apps/sighter/docs/diagrams': 'apps/sighter/docs/images/diagrams/svg', // its export script is run with out-dir docs/images/diagrams
  'docs/images/src/drawio': 'docs/images', // integrations.mjs copies the two family drawings out
};

/** Folders whose .drawio files are sources only and have no committed export on purpose. */
export const NO_EXPORT = ['diagrams/templates'];

/**
 * The committed export of a source: the table's folder, else `<dir>/svg/<name>.svg`. Null only for a folder declared in NO_EXPORT;
 * any other source gets a path, and the caller reports a missing file as an error ("not measured", never a note).
 */
export function committedSvg(drawio) {
  const dir = dirname(drawio);
  if (NO_EXPORT.includes(dir)) return null;
  return `${EXPORT_DIRS[dir] ?? `${dir}/svg`}/${basename(drawio, '.drawio')}.svg`;
}

/** The command that regenerates a source's committed export. */
export function regenerate(drawio, files) {
  const dir = dirname(drawio);
  const name = basename(drawio, '.drawio');
  if (dir === 'docs/images/src/drawio') return 'node docs/images/src/integrations.mjs';
  const app = dir.match(/^(apps\/[^/]+)\/(.*)$/);
  if (app && files.has(`${app[1]}/scripts/export-diagrams.sh`)) {
    const svgDir = EXPORT_DIRS[dir] ?? `${dir}/svg`;
    const out = svgDir.slice(app[1].length + 1).replace(/\/svg$/, '');
    return `(cd ${app[1]} && scripts/export-diagrams.sh ${app[2]} ${out} ${name})`;
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
  let keep = '';
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--root') root = resolve(argv[++i] ?? '');
    else if (argv[i] === '--drawio') drawio = argv[++i] ?? '';
    else if (argv[i] === '--list') list = true;
    else if (argv[i] === '--keep') keep = resolve(argv[++i] ?? '');
    else {
      err('', `svg-drift: unknown argument '${argv[i]}'. Usage: node scripts/svg-drift.mjs [--root <repo>] [--drawio <command>] [--list]`);
      return 2;
    }
  }

  const files = new Set(git(root, 'ls-files', '-z').split('\0').filter(Boolean));
  const sources = [...files].filter((f) => f.endsWith('.drawio')).sort();
  const checked = [];
  const exempt = [];
  let missing = 0;
  for (const src of sources) {
    const svg = committedSvg(src);
    if (!svg) exempt.push(src);
    else if (files.has(svg)) checked.push({ src, svg });
    else {
      missing++;
      err(src, `svg-drift: ${src} has no committed ${svg}, so its export was not measured. Fix: ${regenerate(src, files)} and commit the SVG, or, if the SVGs live elsewhere, declare the folder in EXPORT_DIRS (scripts/svg-drift.mjs); a source without an export on purpose goes in NO_EXPORT.`);
    }
  }
  console.log(`svg-drift: ${sources.length} .drawio sources, ${checked.length} with a committed SVG, ${exempt.length} declared without one, ${missing} missing`);
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
      const out = keep ? join(keep, dir) : join(scratch, String(n));
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
  console.log(`svg-drift: ${same} up to date, ${drifted} drifted (draw.io ${want}, ${viaXvfb ? 'xvfb' : 'display'}, root and sideways translation tolerance ${ROOT_TOLERANCE} px)`);
  return drifted ? 1 : 0;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main());
