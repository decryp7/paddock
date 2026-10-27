#!/usr/bin/env node
// Guards the Tauri shell's least-privilege set-up (AGENTS.md "Security and robustness", docs/suite-lessons.md):
//   * no capability grants a wildcard or the fs / shell / http / opener families to the webview;
//   * the drag-region permissions are granted (a frameless title bar moves only through them);
//   * the CSP has no 'unsafe-eval', no remote origin and no 'unsafe-inline' in script-src;
//   * macOS private API is switched on in tauri.conf.json (the Cargo feature is checked by tauri-build itself);
//   * the macOS bundle is ad-hoc signed (signingIdentity "-"), because SMAppService refuses a linker-signed bundle.
//
//   node scripts/check-capabilities.mjs [app/src-tauri]
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

/** Permissions the window must have. */
export const REQUIRED = ['core:window:allow-start-dragging', 'core:window:allow-internal-toggle-maximize'];
/** Permission prefixes a webview capability must never carry. */
export const FORBIDDEN_PREFIXES = ['fs:', 'shell:', 'http:', 'opener:', 'process:', 'dialog:allow-', 'global-shortcut:'];

/** The permission identifiers of one capability file. */
export function permissionsOf(capability) {
  return (capability.permissions ?? []).map((p) => (typeof p === 'string' ? p : p?.identifier)).filter(Boolean);
}

/** Problems in the capability files (an array of parsed objects). */
export function checkCapabilities(capabilities) {
  const problems = [];
  const all = new Set();
  for (const c of capabilities) {
    for (const p of permissionsOf(c)) {
      all.add(p);
      if (p.includes('*')) problems.push(`capability "${c.identifier}" grants the wildcard permission ${p}`);
      if (FORBIDDEN_PREFIXES.some((f) => p.startsWith(f))) problems.push(`capability "${c.identifier}" grants ${p}: the webview must reach files, shells and the network through app commands only`);
    }
    if (c.remote) problems.push(`capability "${c.identifier}" grants permissions to remote content`);
  }
  for (const r of REQUIRED) if (!all.has(r)) problems.push(`no capability grants ${r} (the frameless title bar could not be dragged)`);
  return problems;
}

/** The CSP as one string, whichever form tauri.conf.json uses. */
export function cspText(csp) {
  if (!csp) return '';
  if (typeof csp === 'string') return csp;
  return Object.entries(csp)
    .map(([k, v]) => `${k} ${Array.isArray(v) ? v.join(' ') : v}`)
    .join('; ');
}

/** Problems in the CSP string. */
export function checkCsp(csp) {
  const problems = [];
  if (!csp.trim()) return ['tauri.conf.json sets no CSP'];
  if (/'unsafe-eval'/.test(csp)) problems.push("the CSP allows 'unsafe-eval'");
  // Remote origins: any http(s)/ws(s) URL except the Tauri IPC origin and loopback.
  for (const m of csp.matchAll(/\b(?:https?|wss?):\/\/[^\s;]+/g)) {
    if (!/^https?:\/\/(ipc\.localhost|localhost|127\.0\.0\.1)(:\d+)?\/?$/.test(m[0])) problems.push(`the CSP names a remote origin: ${m[0]}`);
  }
  if (/(^|;)\s*script-src[^;]*'unsafe-inline'/.test(csp)) problems.push("script-src allows 'unsafe-inline'");
  if (/(^|;)\s*default-src[^;]*\*/.test(csp)) problems.push('default-src uses a wildcard');
  return problems;
}

/** Problems in tauri.conf.json (and the macOS override when present). */
export function checkConfig(conf, macos) {
  const problems = checkCsp(cspText(conf?.app?.security?.csp));
  if (conf?.app?.macOSPrivateApi !== true) problems.push('tauri.conf.json: app.macOSPrivateApi must be true (and the macos-private-api feature set in Cargo.toml)');
  const identity = macos?.bundle?.macOS?.signingIdentity ?? conf?.bundle?.macOS?.signingIdentity;
  if (identity !== '-') problems.push('bundle.macOS.signingIdentity must be "-" (ad-hoc seal): SMAppService refuses a linker-signed bundle');
  return problems;
}

function readJson(path) {
  return JSON.parse(readFileSync(path, 'utf8'));
}

/** Runs every check against an `app/src-tauri` folder; returns the problems. */
export function checkFolder(dir) {
  const capDir = join(dir, 'capabilities');
  const confPath = join(dir, 'tauri.conf.json');
  if (!existsSync(capDir) || !existsSync(confPath)) return [`${dir} has no capabilities/ folder or tauri.conf.json`];
  const caps = readdirSync(capDir)
    .filter((f) => f.endsWith('.json'))
    .map((f) => readJson(join(capDir, f)));
  const macosPath = join(dir, 'tauri.macos.conf.json');
  const macos = existsSync(macosPath) ? readJson(macosPath) : undefined;
  return [...checkCapabilities(caps), ...checkConfig(readJson(confPath), macos)];
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const problems = checkFolder(process.argv[2] ?? 'app/src-tauri');
  for (const p of problems) console.error(`::error::${p}`);
  if (problems.length) process.exit(1);
  console.log('Tauri capabilities and CSP are least-privilege.');
}
