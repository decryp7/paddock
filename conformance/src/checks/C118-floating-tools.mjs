// C118: no compiler or tool in CI floats (ER-26: on 2026-11-12/13 `rustup install stable` moved the Mac and Windows runners from Rust
// 1.99.1 to 1.100.0 during a run and broke three apps' jobs with no code change). Every line of `.gitea/workflows/**` and
// `.gitea/actions/**` (YAML and shell) is judged for: `toolchain: stable|beta|nightly` (without a version or date),
// `rustup toolchain install|default|update stable|beta|nightly`, `dtolnay/rust-toolchain@stable|beta|nightly|master`, `@latest`,
// `npx <package>` without `@<version>` (a binary of a top-level package-lock.json package is pinned by the lockfile), `taiki-e/install-action` `tool:` entries without `@x.y.z`, `taiki-e/install-action` and `dtolnay/rust-toolchain` refs that are not a commit digest, `cargo install` without a pinned version (`--version`, `--tag`, `--rev`, `<crate>@<x.y.z>`) or
// without `--locked`, `curl ... | sh` (or `iwr ... | iex`) with neither a checksum on the line nor a version in the URL, and
// `node-version: lts/*|latest|node|current`. A line that is a comment is skipped. A justified case carries
// `conformance: floating-tool — <why>` (any comment form) on the line or the line above. ER-41: report-only (warnings) until its first
// green run on main; flip SEVERITY to `error` and `mode` to 'gate' then (docs/gates.toml, C118).
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, posix } from 'node:path';
import { warn } from '../finding.mjs';

/** The severity of a finding: report-only for now (ER-41). */
export const SEVERITY = warn;
const WAIVER = /conformance:\s*floating-tool\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*floating-tool/;
const ROOTS = ['.gitea/workflows', '.gitea/actions'];
const CHANNEL = '(?:stable|beta|nightly)';

const FIX = {
  toolchain: 'Pin the toolchain to an exact version, for example `toolchain: 1.99.1` or `nightly-2026-11-01`, or read it from `rust-toolchain.toml`.',
  rustup: 'Install the exact version the repository pins (`rustup toolchain install 1.99.1 --profile minimal`), never a channel name.',
  latest: 'Name an exact version (`pkg@1.2.3`) instead of `latest`.',
  npx: 'Run `npx <package>@<exact version>`, or use the package from the lockfile (`npm exec` in a workspace that lists it).',
  cargo: 'Install with `cargo install <crate> --version <x.y.z> --locked`.',
  curl: 'Download a versioned URL and check its sha256 before running it, or use a pinned action or image.',
  tool: 'Name the exact version, `tool: cargo-hakari@0.9.30`.',
  action: 'Pin the action to a full commit digest with the tag in a trailing comment (`@<40 hex> # v2`).',
  node: 'Pin `node-version` to an exact version, or use `node-version-file: .nvmrc`.',
};

/** The findings of one line of text: [{ kind, what }] (comments and waivers are handled by the caller). */
export function floatingOn(line, lockBins = new Set()) {
  const out = [];
  const t = line.trim();
  if (t.startsWith('#')) return out;
  if (new RegExp(`^(?:-\\s*)?toolchain:\\s*["']?${CHANNEL}["']?\\s*(?:#.*)?$`).test(t)) out.push({ kind: 'toolchain', what: `\`${t.replace(/^-\s*/, '')}\` follows a moving channel` });
  if (new RegExp(`\\brustup\\s+(?:toolchain\\s+install|install|default|update)\\s+(?:--\\S+\\s+)*${CHANNEL}\\b(?!-\\d)`).test(t)) out.push({ kind: 'rustup', what: 'rustup installs a moving channel' });
  if (/\brustup\s+update\s*(?:$|[;&|])/.test(t)) out.push({ kind: 'rustup', what: '`rustup update` moves every installed toolchain' });
  if (new RegExp(`\\buses:\\s*dtolnay/rust-toolchain@(?:${CHANNEL}|master)\\b`).test(t)) out.push({ kind: 'toolchain', what: 'dtolnay/rust-toolchain at a channel ref installs a moving channel' });
  if (/@latest\b/.test(t)) out.push({ kind: 'latest', what: '`@latest` is whatever was published last' });
  // npx runs a binary of a top-level lockfile package from node_modules (pinned by package-lock.json); any other name is downloaded (-y/--yes) at whatever version is newest.
  const npx = /\bnpx\s+((?:(?:-y|--yes|--no-install|--prefer-offline|-q|--quiet)\s+)*)(\S+)/.exec(t);
  if (npx && !/--no-install/.test(npx[1]) && !/^[$"'<(]/.test(npx[2]) && !/.@[^\s]/.test(npx[2])) {
    if (!lockBins.has(npx[2])) out.push({ kind: 'npx', what: `\`npx ${npx[2]}\` has no version and is not a package of the root package-lock.json` });
  }
  const tool = /^(?:-\s*)?tool:\s*["']?([^"'#]+?)["']?\s*(?:#.*)?$/.exec(t);
  if (tool) for (const name of tool[1].split(',').map((x) => x.trim()).filter(Boolean)) if (!/.@\d/.test(name)) out.push({ kind: 'tool', what: `install-action tool \`${name}\` has no \`@x.y.z\`` });
  const dl = /\buses:\s*(taiki-e\/install-action|dtolnay\/rust-toolchain)@(\S+)/.exec(t);
  if (dl && !/^[0-9a-f]{40}$/.test(dl[2]) && !out.some((o) => o.kind === 'toolchain')) out.push({ kind: 'action', what: `\`${dl[1]}@${dl[2]}\` downloads tools and is not pinned to a commit digest` });
  const cargo = /\bcargo\s+install\b(.*)/.exec(t);
  if (cargo && !/--path\b/.test(cargo[1])) {
    const pinned = /--(?:version|tag|rev)\b/.test(cargo[1]) || /\s[\w-]+@\d/.test(cargo[1]);
    if (!pinned) out.push({ kind: 'cargo', what: '`cargo install` has no pinned version' });
    if (!/--locked\b/.test(cargo[1])) out.push({ kind: 'cargo', what: '`cargo install` has no `--locked`' });
  }
  if (/\b(?:curl|wget)\b[^|]*\|\s*(?:sudo\s+)?(?:ba|z)?sh\b/.test(t) || /\b(?:iwr|irm|Invoke-WebRequest|Invoke-RestMethod)\b[^|]*\|\s*iex\b/i.test(t)) {
    if (!/sha256|shasum|checksum/i.test(t) && !/\/v?\d+\.\d+\.\d+/.test(t)) out.push({ kind: 'curl', what: 'a downloaded installer runs with no version in its URL and no checksum' });
  }
  if (/^(?:-\s*)?node-version:\s*["']?(?:lts\/[^"'\s]*|latest|node|current|\*)["']?\s*(?:#.*)?$/.test(t)) out.push({ kind: 'node', what: `\`${t.replace(/^-\s*/, '')}\` follows a moving release line` });
  return out;
}

/** The floating tools of one file: [{ line, kind, what, waived: 'reason' | 'no-reason' | undefined }]. */
export function floatingTools(text, lockBins = new Set()) {
  const lines = text.split('\n');
  const found = [];
  lines.forEach((l, i) => {
    for (const f of floatingOn(l, lockBins)) {
      const near = `${lines[i - 1] ?? ''}\n${l}`;
      const w = WAIVER.exec(near);
      found.push({ line: i + 1, ...f, waived: w ? w[1] : BARE_WAIVER.test(near) ? 'no-reason' : undefined });
    }
  });
  return found;
}

function walk(root, dir, out) {
  let names;
  try { names = readdirSync(join(root, dir)); } catch { return; }
  for (const n of names) {
    const rel = posix.join(dir, n);
    if (statSync(join(root, rel)).isDirectory()) walk(root, rel, out);
    else if (/\.(?:ya?ml|sh|ps1|mjs)$/.test(n)) out.push(rel);
  }
}

/** The binary names of the top-level packages of package-lock.json (and the package names themselves), as `npx` finds them in node_modules. */
export function lockfileBins(lockText) {
  const bins = new Set();
  try {
    for (const [k, v] of Object.entries(JSON.parse(lockText).packages ?? {})) {
      const m = /^node_modules\/((?:@[^/]+\/)?[^/]+)$/.exec(k);
      if (!m) continue;
      bins.add(m[1]);
      if (typeof v.bin === 'string') bins.add(m[1].replace(/^@[^/]+\//, ''));
      else for (const b of Object.keys(v.bin ?? {})) bins.add(b);
    }
  } catch { /* no usable lockfile: every npx without a version is reported */ }
  return bins;
}

export default {
  id: 'C118',
  scope: 'root',
  mode: 'report-only',
  title: 'No compiler or tool in CI floats',
  rule: 26,
  run(ctx) {
    const files = [];
    for (const r of ROOTS) walk(ctx.root, r, files);
    let lock = '';
    try { lock = readFileSync(join(ctx.root, 'package-lock.json'), 'utf8'); } catch { /* reported through the npx findings */ }
    const lockBins = lockfileBins(lock);
    const findings = [];
    for (const file of files.sort()) {
      for (const f of floatingTools(readFileSync(join(ctx.root, file), 'utf8'), lockBins)) {
        if (f.waived && f.waived !== 'no-reason') continue;
        const bare = f.waived === 'no-reason' ? ' (the waiver comment has no reason)' : '';
        findings.push(SEVERITY('C118', 26, file, f.line, `${f.what}${bare} (ER-26: every compiler and tool CI uses is pinned to an exact version)`, `${FIX[f.kind]} A justified case carries \`# conformance: floating-tool — <why>\`.`));
      }
    }
    return findings;
  },
};
