// C97: VMs are started, reverted, stopped and edited only through the broker (ADR 0054 §1.5, `./dev vm`). On 2026-11-04 two
// Windows runs started and reverted the same VMware Fusion guest at once, an old `.vmx` was restored over the current one, and a run
// started the Fusion GUI over a guest with stale `.lck` folders and put a dialog on the user's desktop. Each runner drove the VM
// tools itself and the only guard was an ad hoc `plock` lock a runner could skip. The broker holds one lease per VM, runs a preflight,
// starts headless and is the only writer of a `.vmx`.
// The check reads the scripts and tools (`scripts/`, `tools/`, outside `tools/paddock-dev/src/vm/`) and fails on:
// - a call to `vmrun`, `vmcli`, `prlctl` or `VBoxManage` (any mention outside a comment);
// - `tart run`, `tart clone`, `tart delete`, `tart stop` or `tart set` (`tart exec`, `ip`, `list`, `pull` are reads or guest commands);
// - a write to a `*.vmx` (write, append, rename, copy, move, delete, `sed -i`, a redirect).
// Tests, fixtures and docs are not read. A justified site (a one-time provisioning script with no running guest) carries
// `conformance: vm-broker — <reason>` on the line or the line above. Limits: a command built at run time from pieces is not seen.
import { error } from '../finding.mjs';

const WAIVER = /conformance:\s*vm-broker\s*[—–-]+\s*(\S.{2,})/;
const BARE_WAIVER = /conformance:\s*vm-broker/;
const SCANNED = /^(?:scripts|tools)\/.+\.(?:mjs|cjs|js|ts|sh|bash|ps1|py|ya?ml|rs)$/;
const EXEMPT = /(?:^|\/)(?:tests?|fixtures)\/|\.(?:test|spec)\.[a-z]+$|^tools\/paddock-dev\/src\/vm\//;
const COMMENT = /^\s*(?:#|\/\/|\*|\/\*|--)/;

const TOOLS = /(?<![\w-])(vmrun|vmcli|prlctl|VBoxManage)(?![\w-])/;
const TART = /(?:\btart\b|\$\{?TART\}?|\btartBin\b)['"]?\s*[,(\[]?\s*[\[(]?\s*['"]?(run|clone|delete|stop|set)(?![\w-])/;
const VMX_WRITE = /(?:writeFile(?:Sync)?|appendFile(?:Sync)?|renameSync|copyFile(?:Sync)?|cpSync|rmSync|unlinkSync|\bcp\b|\bmv\b|\brm\b|\bsed\s+-i|\btee\b)\b[^\n]*vmx|>>?\s*["'$\w/.{}-]*\.vmx\b|>>?\s*["']?\$\{?\w*vmx\w*\}?["']?\s*(?:<<|$)/i;

/** The VM-tool uses in one source: [{ line, what, waived: 'reason' | 'no-reason' | undefined }]. */
export function vmToolUses(text) {
  const lines = text.split('\n');
  const hits = [];
  lines.forEach((raw, i) => {
    if (COMMENT.test(raw)) return;
    const line = raw.replace(/\s+(?:\/\/|#)\s.*$/, (m) => (WAIVER.test(m) || BARE_WAIVER.test(m) ? '' : m));
    let what;
    let m;
    if ((m = TOOLS.exec(line))) what = `calls \`${m[1]}\``;
    else if ((m = TART.exec(line))) what = `runs \`tart ${m[1]}\``;
    else if (VMX_WRITE.test(line)) what = 'writes a .vmx';
    if (!what) return;
    const near = [raw, lines[i - 1] ?? ''].join('\n');
    hits.push({ line: i + 1, what, waived: WAIVER.test(near) ? 'reason' : BARE_WAIVER.test(near) ? 'no-reason' : undefined });
  });
  return hits;
}

export default {
  id: 'C97',
  title: 'VMs only through the broker',
  rule: 54,
  run(ctx) {
    const out = [];
    const fix = 'Use the broker: `./dev vm lease <vm> --for <purpose>`, then `./dev vm start|stop|revert|snapshot|vnc|clone|delete <lease>` (scripts/vm-smoke/broker.mjs from Node). A one-time provisioning script with no running guest carries `conformance: vm-broker — <reason>`.';
    for (const f of ctx.files()) {
      if (!SCANNED.test(f) || EXEMPT.test(f)) continue;
      const text = ctx.read(f);
      if (!text || !/vmrun|vmcli|prlctl|VBoxManage|tart|TART|vmx/.test(text)) continue;
      for (const u of vmToolUses(text)) {
        if (u.waived === 'reason') continue;
        if (u.waived === 'no-reason') out.push(error('C97', 54, f, u.line, 'the vm-broker waiver gives no reason', 'Write `conformance: vm-broker — <why no guest runs or is changed here>`.'));
        else out.push(error('C97', 54, f, u.line, `${u.what} outside the VM broker`, fix));
      }
    }
    return out;
  },
};
