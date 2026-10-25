// C37: one integration-test binary per crate, and every workspace member wired up (rule 40, docs/dev-setup.md "Tests" and
// "Builds"). Recurring pattern: new test files landed as `tests/<name>.rs` (a binary each, linked separately), and new crates
// landed without a CI step or the workspace-hack dependency, noticed only by publish.test.mjs or a slow build.
//   (a) A crate's integration tests are modules of `tests/it/main.rs`. A `tests/<name>.rs` beside it is an error unless
//       `ownTestBinaries` of `paddock.conformance.json` names it with the reason it needs a process of its own (a feature it
//       needs, a process-wide log or hook, no test harness). A listed file that no longer exists warns.
//   (b) Every member of a Cargo workspace is tested by a workflow step (`-p <name>` in a cargo test/nextest/llvm-cov command,
//       or a `--workspace` one that does not `--exclude` it, unless the workspace is in `perCrateSteps`), and, in a workspace with cargo-hakari
//       (`.config/hakari.toml`), depends on the workspace-hack crate.
import { error, warn } from '../finding.mjs';

const TEST_COMMAND = /cargo\s+(?:\+\S+\s+)?(?:test|nextest\s+run|llvm-cov)\b[^\n]*/g;

/** The `[package] name` of a Cargo.toml text, or null (a virtual workspace manifest has none). */
export function packageName(text) {
  const m = /^\[package\][^[]*?^name\s*=\s*"([^"]+)"/ms.exec(text ?? '');
  return m ? m[1] : null;
}

/** The member paths (globs as written) of a `[workspace]` table, or null when the manifest has no workspace. */
export function workspaceMembers(text) {
  const ws = /^\[workspace\]([\s\S]*?)(?=^\[[^\]]+\]\s*$|(?![\s\S]))/m.exec(text ?? '');
  if (!ws) return null;
  const members = /members\s*=\s*\[([\s\S]*?)\]/.exec(ws[1]);
  if (!members) return [];
  return [...members[1].replace(/#[^\n]*/g, '').matchAll(/"([^"]+)"/g)].map((m) => m[1]);
}

/** The `hakari-package` of a `.config/hakari.toml` text, or null. */
export function hakariPackage(text) {
  const m = /^hakari-package\s*=\s*"([^"]+)"/m.exec(text ?? '');
  return m ? m[1] : null;
}

/** Whether a workflow command text tests package `name` (`-p name`, `--package name`, `--package=name`). */
export function namesPackage(command, name) {
  const esc = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(`(?:^|\\s)(?:-p|--package)(?:\\s+|=)${esc}(?=\\s|$)`).test(command);
}

/** Whether a `--workspace` workflow command text tests package `name`: it does unless it names it in `--exclude`. */
export function workspaceTests(command, name) {
  if (!/\s--workspace\b/.test(command)) return false;
  const esc = name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return !new RegExp(`(?:^|\\s)--exclude(?:\\s+|=)${esc}(?=\\s|$)`).test(command);
}

const join = (dir, p) => (dir ? `${dir}/${p}` : p);
const dirOf = (f) => (f.includes('/') ? f.slice(0, f.lastIndexOf('/')) : '');

export default {
  id: 'C37',
  title: 'One integration-test binary per crate; every workspace member tested in CI and on the workspace hack',
  rule: 40,
  run(ctx) {
    const out = [];
    const files = ctx.files();
    const manifests = files.filter((f) => f === 'Cargo.toml' || f.endsWith('/Cargo.toml'));
    const allowed = ctx.config.ownTestBinaries ?? {};

    // (a) tests/<name>.rs outside tests/it.
    for (const m of manifests) {
      if (!packageName(ctx.read(m))) continue;
      const dir = dirOf(m);
      const prefix = join(dir, 'tests/');
      for (const f of files) {
        if (!f.startsWith(prefix) || !f.endsWith('.rs') || f.slice(prefix.length).includes('/')) continue;
        const reason = allowed[f];
        if (typeof reason === 'string' && reason.trim()) continue;
        if (f in allowed) {
          out.push(error('C37', 40, 'paddock.conformance.json', 1, `ownTestBinaries lists ${f} without a reason`, 'Say why it needs a test binary of its own (a feature it needs, a process-wide log or hook, no test harness).'));
          continue;
        }
        out.push(error('C37', 40, f, 1, `${f} is a test binary of its own: a crate's integration tests are modules of ${prefix}it/main.rs`, `Move it to ${prefix}it/ and add a \`mod\` line to main.rs; if it needs a process of its own, list it in ownTestBinaries of paddock.conformance.json with the reason.`));
      }
    }
    for (const f of Object.keys(allowed)) {
      if (!ctx.exists(f)) out.push(warn('C37', 40, 'paddock.conformance.json', 1, `ownTestBinaries lists ${f}, which does not exist`, 'Remove the entry.'));
    }

    // (b) workspace members: a CI step, the workspace-hack dependency.
    const commands = ctx.workflows().flatMap((w) => [...w.text.matchAll(TEST_COMMAND)].map((m) => m[0]));
    const perCrate = new Set(ctx.config.perCrateSteps ?? []);
    for (const root of manifests) {
      const members = workspaceMembers(ctx.read(root));
      if (!members) continue;
      const wsDir = dirOf(root);
      const hack = hakariPackage(ctx.read(join(wsDir, '.config/hakari.toml')));
      const memberDirs = members.flatMap((p) => {
        const full = join(wsDir, p);
        if (!full.endsWith('/*')) return [full];
        const base = full.slice(0, -2);
        return manifests.filter((x) => dirOf(dirOf(x)) === base).map(dirOf);
      });
      const wholeWorkspace = !perCrate.has(wsDir);
      for (const d of memberDirs) {
        const manifest = join(d, 'Cargo.toml');
        const text = ctx.read(manifest);
        const name = packageName(text);
        if (!name || name === hack) continue;
        if (!commands.some((c) => namesPackage(c, name) || (wholeWorkspace && workspaceTests(c, name)))) {
          out.push(error('C37', 40, manifest, 1, `${name} is a workspace member that no workflow tests`, `Add a CI step for it (\`cargo nextest run -p ${name}\`, as the other members have).`));
        }
        if (hack && !new RegExp(`^${hack.replace(/[-]/g, '\\-')}(?:\\.workspace)?\\s*=`, 'm').test(text ?? '')) {
          out.push(error('C37', 40, manifest, 1, `${name} does not depend on ${hack}, so it builds its dependencies with its own feature set`, 'Run `cargo hakari generate && cargo hakari manage-deps` in the workspace.'));
        }
      }
    }
    return out;
  },
};
