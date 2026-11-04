import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { vmToolUses } from '../src/checks/C97-vm-broker.mjs';
import { findingsOf, makeApp } from './helpers.mjs';

const lines = (text) => vmToolUses(text).map((u) => u.line);
const fixture = readFileSync(fileURLToPath(new URL('./fixtures/c97-run-win-before.mjs.txt', import.meta.url)), 'utf8');

test('C97 fixture: the runners of 2026-11-04 (vmrun through vm-win.sh, vmx writes, tart clone/run/stop/delete) are all found', () => {
  const what = vmToolUses(fixture).map((u) => u.what);
  assert.ok(what.includes('writes a .vmx'), what.join('; '));
  assert.ok(what.includes('runs `tart clone`') && what.includes('runs `tart run`') && what.includes('runs `tart delete`'), what.join('; '));
});

test('C97 bad: each tool and each form', () => {
  assert.deepEqual(lines('"$lib/vmrun" -T fusion start "$vmx" nogui\n'), [1]);
  assert.deepEqual(lines('sh(`${FUSION}/vmrun`, [])\n'), [1]);
  assert.deepEqual(lines('"$lib/vmcli" "$vmx" ConfigParams SetEntry x\nprlctl start vm\nVBoxManage startvm x\n'), [1, 2, 3]);
  assert.deepEqual(lines("sh('tart', ['stop', vm], 120_000);\n"), [1]);
  assert.deepEqual(lines("await tart(['delete', vm], { timeoutMs: 1 });\n"), [1]);
  assert.deepEqual(lines("spawn('tart', ['run', '--no-graphics', vm]);\n"), [1]);
  assert.deepEqual(lines('tart clone base "$vm"\n"$TART" set "$vm" --cpu 4\n'), [1, 2]);
  assert.deepEqual(lines("writeFileSync(VMX, fixDiskName(text));\n"), [1]);
  assert.deepEqual(lines('cp "$backup" "$dir/vm.vmx"\nsed -i "s/a/b/" w.vmx\necho x > "$dir/w.vmx"\ncat >"$vmx" <<EOF\n'), [1, 2, 3, 4]);
});

test('C97 good: reads, guest commands, comments and messages that do not run a tool', () => {
  assert.deepEqual(lines("sh('tart', ['exec', vm, 'bash', '-c', 'systemctl stop x']);\ntart(['ip', vm]);\ntart(['list', '--quiet']);\ntart pull ghcr.io/x\n"), []);
  assert.deepEqual(lines('// vmrun list, tart stop vm, writeFileSync(VMX)\n# vmrun start\nconst start = 1;\nconst vmxText = readFileSync(VMX, "utf8");\n'), []);
  assert.deepEqual(lines('node ./dev vm start "$lease"\n./dev vm revert "$lease" --to s\n'), []);
});

test('C97 waivers: with a reason the site passes, without one it is reported', () => {
  assert.equal(vmToolUses('# conformance: vm-broker — provisioning, no guest runs\n"$lib/vmcli" "$vmx" x\n')[0].waived, 'reason');
  assert.equal(vmToolUses('"$lib/vmcli" x # conformance: vm-broker\n')[0].waived, 'no-reason');
});

test('C97 through the runner: scripts and tools are read, tests, fixtures and the broker itself are not', async () => {
  const bad = "sh('tart', ['clone', B, vm]);\n";
  const f = await findingsOf(
    'C97',
    makeApp({
      'scripts/vm-smoke/run.mjs': bad,
      'tools/x.sh': 'vmrun list\n',
      'scripts/vm-smoke/run.test.mjs': bad,
      'tools/test/y.mjs': bad,
      'tools/paddock-dev/src/vm/tart.rs': 'let _ = Command::new("tart").arg("clone");\n"vmrun"\n',
      'docs/x.mjs': bad,
      'scripts/ok.sh': '# conformance: vm-broker — one-time disk creation\nvmrun list\n',
      'scripts/bare.sh': '# conformance: vm-broker\nvmrun list\n',
    }),
  );
  assert.deepEqual(f.map((x) => [x.file, x.severity]).sort(), [['scripts/bare.sh', 'error'], ['scripts/vm-smoke/run.mjs', 'error'], ['tools/x.sh', 'error']]);
  assert.match(f.find((x) => x.file === 'tools/x.sh').fix, /dev vm lease/);
  assert.match(f.find((x) => x.file === 'scripts/bare.sh').message, /no reason/);
});
