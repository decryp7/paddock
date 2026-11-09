// C79: every binary that can start supervised children calls `paddock_proc::guardian::run_if_requested()` in `main`
// (ADR 0076 amendment 2026-11-09, the orphan guardian). The guardian is that binary run again with a reserved variable;
// a binary without the call never starts one (it logs that the guard is off), so if it is killed its children's trees
// run on (a headless browser left by a killed test process on CI ran for 2.5 h).
// Which binaries: every `src/main.rs`, `src/bin/*.rs`, `src/bin/*/main.rs` and `[[bin]] path` of a package whose
// dependency closure in its workspace's Cargo.lock reaches `paddock-proc`. The call must be the first statement of
// `fn main` (comments do not count): the guardian must start before the app does anything else. Test binaries (whose `main` is the harness) opt in with
// `enable_in_test_binary` and are not checked here. ADOPTING lists the mains that do not call it yet; it may only shrink:
// a main missing from it is an error, a listed one is a warning, and a listed one that calls it already is an error until
// it leaves the list. A `#![no_main]` binary (a fuzz target) has no `main` and is skipped.
import { error, warn } from '../finding.mjs';
import { maskRust } from './C39-freed-port.mjs';

/** `fn main` (any attributes, `async`, a return type), then its body's first statement. */
const MAIN_FN = /\bfn\s+main\s*\(\s*\)[^{;]*\{/;
const FIRST_CALL = /^\s*(?:::)?paddock_proc\s*::\s*guardian\s*::\s*run_if_requested\s*\(\s*\)\s*;/;

/** An `async fn main`, or a `main` under an attribute that wraps it (`#[tokio::main]`, `#[async_std::main]`, ...). */
const WRAPPED_MAIN = /(?:#\[[^\]]*\bmain\b[^\]]*\]\s*)+(?:pub\s+)?(?:async\s+)?fn\s+main\b|\basync\s+fn\s+main\b/;

/**
 * How `text`'s `main` calls the guardian (comments and strings masked, so a commented-out call does not count): `hooked`
 * (a plain `fn main` whose first statement is the call), `wrapped` (an `async` or attribute-wrapped `main`: a runtime
 * starts before its body, so the call must move to a plain `fn main`), or `missing`.
 */
export function mainState(text) {
  const masked = maskRust(text);
  if (WRAPPED_MAIN.test(masked)) return 'wrapped';
  const m = MAIN_FN.exec(masked);
  return m !== null && FIRST_CALL.test(masked.slice(m.index + m[0].length)) ? 'hooked' : 'missing';
}

/** Whether `text`'s plain `fn main` begins with the call. */
export const callsFirst = (text) => mainState(text) === 'hooked';
const MAIN = /(^|\/)src\/(main\.rs|bin\/[^/]+\.rs|bin\/[^/]+\/main\.rs)$/;

/** Mains that reach paddock-proc and do not call the guardian yet, each with why; adoption is per app, in its release window. */
export const ADOPTING = new Map(
  [
    ['apps/debrief/app/src-tauri/src/main.rs', 'Debrief app: adopts in 1.2'],
    ['apps/debrief/crates/cargo-debrief-plugin/src/main.rs', 'Debrief: adopts in 1.2'],
    ['apps/debrief/crates/debrief-agent/src/bin/debrief-agent.rs', 'Debrief: adopts in 1.2'],
    ['apps/debrief/crates/debrief-ai-worker/src/bin/fake_llama_server.rs', 'Debrief test fixture: adopts in 1.2'],
    ['apps/debrief/crates/debrief-ai-worker/src/main.rs', 'Debrief: adopts in 1.2'],
    ['apps/debrief/crates/debrief-ai/src/bin/fake_worker.rs', 'Debrief test fixture: adopts in 1.2'],
    ['apps/debrief/crates/debrief-cli/src/bin/debrief.rs', 'Debrief: adopts in 1.2'],
    ['apps/debrief/crates/debrief-collect/src/bin/debrief-collect.rs', 'Debrief: adopts in 1.2'],
    ['apps/debrief/crates/debrief-ipc/src/bin/fake_child.rs', 'Debrief test fixture: adopts in 1.2'],
    ['apps/debrief/crates/debrief-monitor/src/bin/debrief-monitor.rs', 'Debrief: adopts in 1.2'],
    ['apps/debrief/crates/debrief-plugin-e2e/src/kv.rs', 'Debrief test plugin: adopts in 1.2'],
    ['apps/debrief/crates/debrief-plugin-e2e/src/store_worker.rs', 'Debrief test plugin: adopts in 1.2'],
    ['apps/debrief/crates/debrief-plugin-host/src/bin/fake_plugin.rs', 'Debrief test fixture: adopts in 1.2'],
    ['apps/debrief/crates/debrief-plugin-test/src/bin/fixture.rs', 'Debrief test fixture: adopts in 1.2'],
    ['apps/debrief/crates/debrief-plugins/src/main.rs', 'Debrief: adopts in 1.2'],
    ['apps/debrief/crates/debrief-store-worker/src/main.rs', 'Debrief: adopts in 1.2'],
    ['apps/debrief/crates/debrief-video/src/bin/fake_ffmpeg.rs', 'Debrief test fixture: adopts in 1.2'],
    ['apps/debrief/examples/plugins/annotator-example/src/main.rs', 'Debrief example plugin: adopts in 1.2'],
    ['apps/debrief/examples/plugins/collector-example/src/main.rs', 'Debrief example plugin: adopts in 1.2'],
    ['apps/debrief/examples/plugins/importer-example/src/main.rs', 'Debrief example plugin: adopts in 1.2'],
    ['apps/debrief/examples/plugins/tool-example/src/main.rs', 'Debrief example plugin: adopts in 1.2'],
    ['apps/debrief/examples/plugins/transform-example/src/main.rs', 'Debrief example plugin: adopts in 1.2'],
    ['apps/dyno/app/src-tauri/src/main.rs', 'Dyno app: adopts in its next release'],
    ['apps/dyno/crates/dyno-agent/src/main.rs', 'Dyno: adopts in its next release'],
    ['apps/dyno/crates/dyno-cli/src/main.rs', 'Dyno: adopts in its next release'],
    ['apps/dyno/crates/dyno-helper/src/main.rs', 'Dyno helper runs as root: the guardian is off for root binaries (ADR 0076 amendment 2026-11-09), so it does not adopt'],
    ['apps/dyno/crates/dyno-plugin-host/src/bin/test_plugin.rs', 'Dyno test fixture: adopts in its next release'],
    ['apps/dyno/plugins/dyno-plugin-example/src/main.rs', 'Dyno example plugin: adopts in its next release'],
    ['apps/pitcrew/app/src-tauri/src/main.rs', 'Pitcrew app: adopts in 1.1'],
    ['apps/pitcrew/crates/pitcrew-service/src/main.rs', 'Pitcrew: adopts in 1.1'],
    ['apps/pitwall/app/src-tauri/src/main.rs', 'Pitwall app: adopts in 1.1'],
    ['apps/pitwall/crates/pitwall-cli/src/main.rs', 'Pitwall: adopts in 1.1'],
    ['apps/pitwall/crates/pitwall-server/src/fake_engine_main.rs', 'Pitwall test fixture (async main): adopts in 1.1'],
    ['apps/pitwall/crates/pitwall-server/src/main.rs', 'Pitwall (async main, needs a plain fn main first): adopts in 1.1'],
    ['apps/shell-demo/src-tauri/agent/src/main.rs', 'shell demo: adopts with the next shared batch'],
    ['apps/shell-demo/src-tauri/src/main.rs', 'shell demo: adopts with the next shared batch'],
    ['apps/spotter/app/src-tauri/src/main.rs', 'Spotter app: adopts in its next release'],
    ['apps/workbench-demo/src-tauri/src/main.rs', 'workbench demo: adopts with the next shared batch'],
    ['crates/cargo-paddock-plugin/src/main.rs', 'shared tool: adopts with the next shared batch'],
    ['crates/paddock-ipc/src/bin/test_child.rs', 'shared test fixture: adopts with the next shared batch'],
    ['crates/paddock-plugin-test/src/bin/fixture.rs', 'shared test fixture: adopts with the next shared batch'],
    ['crates/paddock-plugin-test/src/bin/fixture_v1.rs', 'shared test fixture: adopts with the next shared batch'],
    ['crates/paddock-plugin-test/src/bin/misbehaving.rs', 'shared test fixture: adopts with the next shared batch'],
    ['crates/paddock-setup/src/main.rs', 'shared tool: adopts with the next shared batch'],
    ['tools/heavy/src/main.rs', 'heavy tool: adopts with the next tooling batch'],
    ['tools/paddock-dev/src/main.rs', 'paddock-dev: adopts with the next shared batch'],
    ['tools/wiki/src/main.rs', 'wiki tool: adopts with the next tooling batch'],
  ],
);

/** `{ name: [dependency names] }` from a Cargo.lock text. */
export function lockGraph(text) {
  const graph = {};
  for (const block of text.split(/^\[\[package\]\]\s*$/m).slice(1)) {
    const name = /^name = "([^"]+)"/m.exec(block)?.[1];
    if (!name) continue;
    const deps = /^dependencies = \[([\s\S]*?)\]/m.exec(block)?.[1] ?? '';
    graph[name] = [...deps.matchAll(/"([^" ]+)[^"]*"/g)].map((m) => m[1]);
  }
  return graph;
}

/** Whether `name` reaches `target` in `graph`. */
export function reaches(graph, name, target = 'paddock-proc', seen = new Set()) {
  if (name === target) return true;
  if (seen.has(name)) return false;
  seen.add(name);
  return (graph[name] ?? []).some((d) => reaches(graph, d, target, seen));
}

const dirOf = (p) => (p.includes('/') ? p.slice(0, p.lastIndexOf('/')) : '');

/** The `[[bin]] path` entries of a Cargo.toml, relative to its folder. */
export function binPaths(manifest) {
  return manifest
    .split(/^\[\[bin\]\]\s*$/m)
    .slice(1)
    .map((block) => /^path\s*=\s*"([^"]+)"/m.exec(block.split(/^\[/m)[0])?.[1])
    .filter((p) => p !== undefined);
}

/** The mains (paths) that must call the guardian: [{ file, hooked }]. `read(path)` gives a file's text or null. */
export function guardedMains(files, read) {
  const have = new Set(files);
  const nearest = (dir, name) => {
    for (let d = dir; ; d = dirOf(d)) {
      const p = d ? `${d}/${name}` : name;
      if (have.has(p)) return p;
      if (!d) return null;
    }
  };
  const graphs = new Map();
  const out = [];
  // Mains by convention, then those a manifest names with `[[bin]] path`.
  const named = files
    .filter((f) => f.endsWith('Cargo.toml'))
    .flatMap((m) => binPaths(read(m) ?? '').map((p) => (dirOf(m) ? `${dirOf(m)}/${p}` : p).replace(/\/\.\//g, '/')));
  const mains = [...new Set([...files.filter((f) => MAIN.test(f)), ...named.filter((f) => have.has(f))])];
  for (const f of mains) {
    if (f.includes('/tests/') || f.includes('/fixture/')) continue;
    // A `#![no_main]` binary (a fuzz target: libfuzzer brings `main`) has no `main` to call it from.
    if (/^\s*#!\[no_main\]/m.test(maskRust(read(f) ?? ''))) continue;
    const manifest = nearest(dirOf(f), 'Cargo.toml');
    const pkg = manifest && /^\[package\][\s\S]*?^name\s*=\s*"([^"]+)"/m.exec(read(manifest) ?? '')?.[1];
    const lock = manifest && nearest(dirOf(manifest), 'Cargo.lock');
    if (!pkg || !lock) continue;
    if (!graphs.has(lock)) graphs.set(lock, lockGraph(read(lock) ?? ''));
    if (pkg === 'paddock-proc' || !reaches(graphs.get(lock), pkg)) continue;
    const state = mainState(read(f) ?? '');
    out.push({ file: f, hooked: state === 'hooked', wrapped: state === 'wrapped' });
  }
  return out;
}

export default {
  id: 'C79',
  title: 'Binaries that start supervised children call the orphan guardian',
  rule: 53,
  scope: 'root',
  mode: 'gate',
  run(ctx, adopting = ADOPTING) {
    const fix = 'Call `paddock_proc::guardian::run_if_requested();` first thing in `main` (ADR 0076 amendment 2026-11-09).';
    const out = [];
    // Paths are relative to the checked root (the repo, or an app's folder in it): a listed path matches by its end.
    const reason = (file) => adopting.get(file) ?? [...adopting].find(([k]) => k.endsWith(`/${file}`))?.[1];
    for (const { file, hooked, wrapped } of guardedMains(ctx.files(), (p) => ctx.read(p))) {
      const why = reason(file);
      const listed = why !== undefined;
      if (hooked && listed) out.push(error('C79', 53, file, 1, 'calls the orphan guardian but is still on ADOPTING', 'Remove it from ADOPTING in C79 (the list only shrinks).'));
      else if (!hooked && listed) out.push(warn('C79', 53, file, 1, `does not call the orphan guardian yet (${why})`, fix));
      else if (wrapped) out.push(error('C79', 53, file, 1, '`main` is async or wrapped by an attribute (`#[tokio::main]`, `#[async_std::main]`): a runtime starts before its first statement, so the orphan guardian cannot be called first', 'Write a plain `fn main` that calls `paddock_proc::guardian::run_if_requested();` first, then builds the runtime and runs the async body on it.'));
      else if (!hooked) out.push(error('C79', 53, file, 1, 'can start supervised children but does not call the orphan guardian first in main: if it is killed, they run on', fix));
    }
    return out;
  },
};
