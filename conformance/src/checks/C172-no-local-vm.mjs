// C172 (docs/rules.md NEW-no-local-vm, user decision 2026-11-26): local gate code never starts a VM or a container on a developer's
// Mac. Linux checks are CI's Linux jobs on the Linux runner, Windows checks the Windows runner. The container rows were the Mac's largest
// load (clippy-linux, tests-linux, warm-linux, golden confirm).
//
// Flagged, in the code the local gates and developer commands run: tools/paddock-dev/src/ (Rust, outside src/gates/ and ci_*.rs, the CI-only
// code) and crates/*/src (golden.rs printed a container command)
// (Rust, outside tests.rs, *_tests.rs and the
// `#[cfg(test)]` tail of a file), tools/githooks/ and lefthook.yml: a string literal or a command word that names docker, podman,
// nerdctl, orb, orbctl, vmrun or `linux-container.sh`. Whole-line comments are not read. CI-only scripts (.gitea/, conformance/ci/,
// tools/linux-container.sh itself) are not in scope. A site that is fine carries `// conformance: no-local-vm — <why>` on its line or
// the line above.
// Positive control (ER-33): the detector first runs on a built-in sample that must be flagged; if it is not, the check is broken.
// ER-41: report-only until its first green run on the real tree, recorded in docs/gates.toml; review date 2026-12-09.
import { error, unwaivable, warn } from '../finding.mjs';

const WAIVER = /conformance:\s*no-local-vm\s*[—–-]+\s*\S.{2,}/;
const TOOLS = 'docker|podman|nerdctl|orbctl|orb|vmrun|linux-container\\.sh';
// A string literal whose text is a tool name or starts a path ending in one: "docker", "tools/linux-container.sh", "/usr/bin/docker info".
const IN_STRING = new RegExp(`"(?:[^"\\n]*/)?(?:${TOOLS})(?:[\\s"])`);
// A shell command word: at the start of a line, or after ; & | ( or $(.
const COMMAND_WORD = new RegExp(`(?:^|[;&|(]|\\$\\()\\s*(?:sudo\\s+)?(?:[\\w./-]*/)?(?:${TOOLS})(?:\\s|$)`);

const IN_SCOPE = /^(?:tools\/paddock-dev\/src\/(?!gates\/|ci_)[^]+\.rs|crates\/[^/]+\/src\/[^]+\.rs|tools\/githooks\/.+|lefthook\.ya?ml)$/;
const TEST_FILE = /(?:^|\/)tests\.rs$|_tests?\.rs$/;

/** The 1-based lines of `text` (file `file`) that start a VM or container. Empty for a file that is not local gate code. */
export function localVmSites(text, file) {
  if (!IN_SCOPE.test(file) || TEST_FILE.test(file)) return [];
  const rust = file.endsWith('.rs');
  let body = text;
  if (rust) {
    const cut = text.search(/^#\[cfg\(test\)\]/m);
    if (cut >= 0) body = text.slice(0, cut);
  }
  const lines = body.split('\n');
  const out = [];
  lines.forEach((l, i) => {
    const t = l.trim();
    if (t.startsWith('//') || t.startsWith('#')) return;
    if (WAIVER.test(l) || WAIVER.test(lines[i - 1] ?? '')) return;
    if (rust ? IN_STRING.test(l) : COMMAND_WORD.test(l)) out.push(i + 1);
  });
  return out;
}

const FIX = 'Remove it: Linux checks are CI\'s Linux jobs on the Linux runner (docs/rules.md NEW-no-local-vm). A site that is fine carries `// conformance: no-local-vm — <why>`.';

export default {
  id: 'C172',
  scope: 'root',
  mode: 'report-only',
  title: 'Local gate code never starts a VM or a container (docker, orb, orbctl, vmrun, linux-container.sh)',
  rule: 55,
  run(ctx) {
    // Positive control: a hidden signal must go unseen only when the check is broken.
    if (!localVmSites('fn a() { Command::new("docker"); }\n', 'tools/paddock-dev/src/pre_push/x.rs').length || !localVmSites('docker run x\n', 'tools/githooks/x.sh').length) {
      return [unwaivable(error('C172', 55, 'conformance/src/checks/C172-no-local-vm.mjs', 1, 'check broken: the detector does not see its own positive control', 'Fix the patterns in C172-no-local-vm.mjs.'))];
    }
    const out = [];
    for (const f of ctx.files()) {
      if (!IN_SCOPE.test(f)) continue;
      const text = ctx.read(f);
      if (!text) continue;
      for (const line of localVmSites(text, f)) {
        out.push(warn('C172', 55, f, line, 'starts a container or a VM in local gate code: no VM or container runs on this Mac (user, 2026-11-26)', FIX));
      }
    }
    return out;
  },
};
