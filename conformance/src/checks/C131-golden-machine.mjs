// C131: a golden file holds nothing that belongs to the machine that captured it (ER-21 class "a golden captured on one machine fails on
// another": WP-2 of train 14 alone had six: macOS-only git config keys, the CI image's /sccache mount, RAM from sysctl against a file,
// the host OS paired with its pin, root against a normal uid, and a guardian golden with the Mac's uid 501 baked in that would have
// turned the Linux leg red; see docs/lessons.md, 2026-11-15). The fix is in the test, not the file: normalise the value in the test's
// `norm()` (tools/paddock-dev/tests/it/host_steps.rs) before the comparison, so the golden says `<uid>` and every machine writes the same.
//
// Read: the tracked text files of the golden folders: any path with a `golden` or `goldens` folder, and files named `*.golden`.
// Flagged, one finding per line and kind:
// - ids: `uid`, `gid`, `euid`, `egid`, `user_id` or `group_id` followed by a number (the Mac's 501 against a CI container's 0);
// - home paths: `/Users/<name>`, `/home/<name>`, `C:\Users\<name>` (also with `\\`; a placeholder such as `<name>` is not a name);
// - temp paths: `/tmp/<x>`, `/private/tmp/<x>`, `/var/folders/`, `\Temp\`;
// - this machine, read from the environment when the check runs and never written down: the user name and home folder, the host name,
//   the RAM in bytes, KiB or MiB, and the CPU count next to `cpu`, `core` or `thread`. `PADDOCK_GOLDEN_FACTS` (JSON: user, home, host,
//   memBytes, cpus) replaces them, for tests and for judging a tree against another machine.
// A site that is fine (an invented id in a fixture, a documented name) has an entry in `conformance/c131-allow.json` (path, a piece of the
// flagged line as `site`, a `reason`, `count`), read by the shared allow list; an entry that matches nothing is stale (a warning).
// Known gaps: a value spelled another way (`0x1f5`, a size in GiB, a uid in `ls -l` columns) and one split over lines; a golden outside the
// folders above is not read.
// ER-41: report-only (warnings) until its first green run on the real tree, recorded in docs/gates.toml.
// Rust host: tools/paddock-dev/src/check/c131.rs (same findings, line for line).
import os from 'node:os';
import { warn } from '../finding.mjs';
import { readAllowList } from '../util/allow-list.mjs';

export const ALLOW = 'conformance/c131-allow.json';
/** The check's own files: they name the patterns. */
export const IMPLEMENTATION = new Set([
  'conformance/src/checks/C131-golden-machine.mjs',
  'conformance/test/c131-golden-machine.test.mjs',
  'tools/paddock-dev/src/check/c131.rs',
  'tools/paddock-dev/tests/it/check_c131.rs',
  ALLOW,
]);
const GOLDEN_PATH = /(?:^|\/)goldens?\/|\.golden$/;
const BINARY = /\.(?:png|jpe?g|gif|ico|icns|webp|bmp|tiff?|pdf|zip|gz|tgz|xz|zst|7z|whl|wasm|dmg|pkg|msi|exe|dll|so|dylib|a|lib|rlib|o|obj|pdb|ttf|otf|woff2?|mp3|mp4|mov|wav|webm|parquet|duckdb|sqlite|db|evtx|dmp|bin|onnx|safetensors)$/i;
const BROKEN_FIX = 'Restore conformance/c131-allow.json: {"allow": [{"path": "...", "site": "...", "reason": "...", "count": 1}]}.';
const FIX = "Normalise it in the test's norm() (replace the value with a placeholder before the comparison) and regenerate the golden; a value that is fine here (invented data) gets an entry in conformance/c131-allow.json with a reason.";

/**
 * The line patterns, each with its kind; group 1 is the site. The text around a site is matched as a character, never with a
 * look-around, so the Rust host's `regex` crate (which has none) reads the same lines. A pattern may match once per line and kind.
 */
const PATTERNS = [
  { kind: 'id', re: /(?:^|[^A-Za-z0-9_])((?:e?uid|e?gid|user_id|group_id)[^A-Za-z0-9_\n][^A-Za-z0-9\n]{0,5}[0-9]+)/i },
  { kind: 'home', re: /(?:^|[^A-Za-z0-9_.-])((?:\/Users\/|\/home\/|[A-Za-z]:\\{1,2}Users\\{1,2})[A-Za-z0-9][A-Za-z0-9._-]*)/ },
  { kind: 'temp', re: /(?:^|[^A-Za-z0-9_.-])((?:\/private)?\/(?:tmp\/[A-Za-z0-9._-]|var\/folders\/))|(\\Temp\\)/ },
];
const escape = (t) => t.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');

/** What this machine is: { user, home, host, memBytes, cpus }, from `PADDOCK_GOLDEN_FACTS` or the environment. */
export function machineFacts(env = process.env) {
  if (env.PADDOCK_GOLDEN_FACTS) return JSON.parse(env.PADDOCK_GOLDEN_FACTS);
  return {
    user: env.USER || env.USERNAME || env.LOGNAME || '',
    home: env.HOME || env.USERPROFILE || '',
    host: os.hostname(),
    memBytes: os.totalmem(),
    cpus: os.availableParallelism(),
  };
}

/** The patterns of the machine `facts` describe: [{ kind, re }], group 1 the site. A fact too short or too common to tell from other text makes none. */
export function machinePatterns(facts) {
  const out = [];
  const word = (name) => new RegExp(`(?:^|[^A-Za-z0-9_.-])(${escape(name)})(?:$|[^A-Za-z0-9_-])`, 'i');
  const user = String(facts.user ?? '');
  if (user.length >= 3 && user.toLowerCase() !== 'root') out.push({ kind: 'user', re: word(user) });
  const home = String(facts.home ?? '');
  if (home.length > 3 && home !== '/' && home !== '/root') out.push({ kind: 'home-of-this-machine', re: new RegExp(`(${escape(home)})(?:$|[^A-Za-z0-9_])`) });
  const host = String(facts.host ?? '').replace(/\.local$/i, '');
  for (const h of new Set([host, host.split('.')[0]])) {
    if (h.length >= 4 && h.toLowerCase() !== 'localhost') out.push({ kind: 'host', re: word(h) });
  }
  const mem = Number(facts.memBytes);
  if (Number.isFinite(mem) && mem >= 2 ** 28) {
    for (const n of new Set([mem, Math.floor(mem / 1024), Math.floor(mem / 1048576)])) out.push({ kind: 'ram', re: new RegExp(`(?:^|[^0-9.])(${n})(?:$|[^0-9])`) });
  }
  const cpus = Number(facts.cpus);
  if (Number.isInteger(cpus) && cpus >= 2) out.push({ kind: 'cpus', re: new RegExp(`(?:^|[^A-Za-z0-9_])((?:cpus?|cores?|threads?|nproc)[^A-Za-z0-9\\n]{0,4}${cpus})(?:$|[^0-9.])`, 'i') });
  return out;
}

/** [{ line, kind, site, text }] of every machine-specific literal in `text`, one per line and kind. */
export function goldenSites(text, facts) {
  const patterns = [...PATTERNS, ...machinePatterns(facts)];
  const out = [];
  text.split('\n').forEach((raw, i) => {
    const line = raw.replace(/\r$/, '');
    for (const { kind, re } of patterns) {
      const m = re.exec(line);
      if (m) out.push({ line: i + 1, kind, site: m[1] ?? m[2], text: line });
    }
  });
  return out;
}

/** Whether `path` is a file the check reads. */
export const isGolden = (path) => GOLDEN_PATH.test(path) && !BINARY.test(path) && !IMPLEMENTATION.has(path);

const SPEC = {
  id: 'C131',
  rule: 40,
  file: ALLOW,
  unseen: 'a machine-specific value',
  restore: BROKEN_FIX,
};

export default {
  id: 'C131',
  scope: 'root',
  mode: 'report-only',
  allowList: SPEC,
  title: 'A golden file holds no value of the machine that captured it',
  rule: 40,
  run(ctx) {
    const { list: allow, broken } = readAllowList(ctx, this.allowList);
    if (broken) return broken;
    const facts = machineFacts();
    const out = [];
    for (const f of ctx.files()) {
      if (!isGolden(f)) continue;
      const { text, finding } = allow.read(f);
      if (finding) {
        out.push(finding);
        continue;
      }
      if (text.includes('\0')) {
        allow.skip(f);
        continue;
      }
      for (const { line, kind, site, text: at } of goldenSites(text, facts)) {
        const said = allow.admit(f, (s) => at.includes(s));
        if (said === 'allowed') continue;
        const what = said === 'beyond' ? ' beyond the count of its allow entry' : '';
        out.push(warn('C131', 40, f, line, `golden holds a machine-specific ${kind} \`${site}\`${what}: it fails on a machine that differs`, FIX));
      }
    }
    return [...out, ...allow.warnings()];
  },
};
