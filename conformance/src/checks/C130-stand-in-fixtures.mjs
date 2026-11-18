// C130: Rust test code does not plant a shell script as a fixture; it installs the testkit's stand-in (`paddock_testkit::StandIn`).
// A fixture written as `#!/bin/sh` (or bash, zsh, through `env`) cannot run on Windows, so its test is `#[cfg(unix)]` or a windows-twin
// waiver, and a hand-written script rarely prints what the real program prints (an npm stand-in that printed nothing hid a `--json` bug).
// ER-21 class "a fixture program that exists only on Unix": train 12's planner control never ran on Windows (42619e558), run 8138 lost
// grep and tr, WP-7 put windows-twin waivers on three unix-only test files. Share first (rule 42): one stand-in binary, written once.
// Flagged, one finding per line, in Rust test code only (a file of a `tests/` folder, `tests.rs`, `*_test.rs`, or a file from its first
// `#[cfg(test)]` / `#[cfg(all(test, ..))]` / `#[cfg(any(.., test))]` line on, which also covers a test-only item in the middle of a
// file; and any file that calls `write_program(`, test-support files included, whole): a line that holds `#!/bin/sh`, `#!/bin/bash`, `#!/usr/bin/env bash` and the like. Comment lines (`//`) are not read.
// Not flagged, said out loud: a script built without a shebang (`sh -c`), a shebang another way (a byte string split over lines), and
// shell files themselves (C129 flags those; this reads the Rust text C129 names as its known gap, so the two never report the same
// line). A fixture that has to be a script (a hook or a wrapper script under test) carries `// conformance: stand-in — <why>` on its
// line or the line above, and is counted: the allow list must name it (site `conformance: stand-in`, a count per file), so a new waiver is growth
// of the list, which the shrink-only rule (util/allow-list.mjs, against the merge base) puts to the user. Today's sites are the allow list `conformance/c130-allow.json` (path, site = the shebang text, reason, count);
// it only ever shrinks by its own rule (a stale entry is a warning: remove it or lower its count), and a site beyond an entry's count
// is reported at its own line. Convert a site to `StandIn` and delete its entry.
// The shared reader owns the error paths (`util/allow-list.mjs`): an unreadable allow list makes the check broken; a file that cannot
// be read is not measured. The check's own files are not read (`IMPLEMENTATION`): their invented lines are test data.
// Report-only (warnings) until its first green run on the real tree is recorded here (ER-41). Recorded: not yet.
// Rust host: tools/paddock-dev/src/check/c130.rs (same findings, line for line; tests/it/check_c130.rs compares them).
import { warn } from '../finding.mjs';
import { parseAllowList, readAllowList } from '../util/allow-list.mjs';

export const ALLOW = 'conformance/c130-allow.json';
/** The check's own files: they name the pattern, their invented lines are test data. */
export const IMPLEMENTATION = new Set([
  'conformance/src/checks/C130-stand-in-fixtures.mjs',
  'conformance/test/c130-stand-in-fixtures.test.mjs',
  'tools/paddock-dev/src/check/c130.rs',
  'tools/paddock-dev/tests/it/check_c130.rs',
  ALLOW,
]);
const SHEBANG = /#!\/(?:usr\/)?bin\/(?:env[ \t]+)?(?:ba|z)?sh\b/;
const WAIVER = /conformance:\s*stand-in\s*[—–-]+\s*\S.{2,}/;
const TEST_PATH = /(?:^|\/)tests?\/|(?:^|\/)tests\.rs$|_tests?\.rs$/;
const NOT_READ = /(?:^|\/)(?:target|vendor|fixtures|node_modules)\//;
const WRITES_PROGRAM = /\bwrite_program\(/;
const CFG_TEST = /^[ \t]*#\[cfg\((?:test|(?:all|any)\([^\]]*\btest\b[^\]]*)\)\]/;
const FIX =
  "Install the testkit's stand-in instead: `paddock_testkit::StandIn::new(dir, \"name\").stdout(..).exit(..).records_args().install()?` (it runs on Windows and records its calls). A fixture that has to be a script carries `// conformance: stand-in — <why>`; an existing site is listed in conformance/c130-allow.json until it is converted.";
const WAIVER_FIX = `Waivers are counted, so none is free: list it in ${ALLOW} (path, site \`conformance: stand-in\`, a reason and the file's count), which the user approves, or convert the fixture to \`StandIn\`.`;
const BEYOND_FIX = `Install the testkit's stand-in instead (\`paddock_testkit::StandIn\`), or raise the entry's \`count\` in ${ALLOW} only for a site that has to stay a script.`;

/** Whether `path` is a Rust file this check reads. */
export const readable = (path) => path.endsWith('.rs') && !NOT_READ.test(path) && !IMPLEMENTATION.has(path);

/** The `site` an allow entry uses to name a waived fixture. */
export const WAIVED_SITE = 'conformance: stand-in';

/** [{ line, text, waived }] of every shebang line in the test code of `text` (the Rust file `path`); `waived` when a comment with a reason is on it or above. */
export function sites(path, text) {
  if (!readable(path)) return [];
  const lines = text.split('\n');
  // A file that calls `write_program(` is a fixture writer wherever it lives (a test-support file such as a `fake.rs` compiled
  // for tests is not under `tests/` and has no `#[cfg(test)]`): every shebang in it counts.
  const whole = TEST_PATH.test(path) || WRITES_PROGRAM.test(text);
  let from = whole ? 0 : lines.findIndex((l) => CFG_TEST.test(l));
  if (from < 0) return [];
  const out = [];
  for (let i = from; i < lines.length; i++) {
    const line = lines[i];
    if (line.trim().startsWith('//') || !SHEBANG.test(line)) continue;
    out.push({ line: i + 1, text: line, waived: WAIVER.test(line) || WAIVER.test(lines[i - 1] ?? '') });
  }
  return out;
}

/** The allow list: [{ path, site, reason, count }], or throws why it cannot be read (the shared parser). */
export const parseAllow = (text) => parseAllowList(text, ALLOW);
const SPEC = { id: 'C130', rule: 42, file: ALLOW, unseen: 'a shell fixture', restore: 'Restore conformance/c130-allow.json: {"allow": [{"path": "...", "site": "#!/bin/sh", "reason": "...", "count": 1}]}.' };

export default {
  id: 'C130',
  scope: 'root',
  mode: 'report-only',
  allowList: SPEC,
  title: 'Rust tests install the testkit stand-in, not a #!/bin/sh fixture',
  rule: 42,
  run(ctx) {
    const { list: allow, broken } = readAllowList(ctx, this.allowList);
    if (broken) return broken;
    const out = [];
    for (const f of ctx.files()) {
      if (!readable(f)) continue;
      const { text, finding } = allow.read(f);
      if (finding) {
        out.push(finding);
        continue;
      }
      for (const { line, text: at, waived } of sites(f, text)) {
        const said = allow.admit(f, waived ? (site) => site === WAIVED_SITE : (site) => at.includes(site));
        if (said === 'allowed') continue;
        if (waived) {
          out.push(warn('C130', 42, f, line, said === 'beyond' ? 'a stand-in waiver beyond the count of its allow entry' : 'a stand-in waiver the allow list does not name', WAIVER_FIX));
          continue;
        }
        out.push(
          said === 'beyond'
            ? warn('C130', 42, f, line, 'writes another shell fixture beyond the count of its allow entry', BEYOND_FIX)
            : warn('C130', 42, f, line, 'a test plants a shell script as a fixture program: it cannot run on Windows and does not say what the real program prints', FIX),
        );
      }
    }
    return [...out, ...allow.warnings()];
  },
};
