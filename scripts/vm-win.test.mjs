// The local Windows test VM tooling (docs/testing/windows-test-vm.md): the answer file, the pins and the guard rails of
// scripts/vm-win.sh and scripts/vm-win/prepare.sh. Nothing here starts a VM or touches Fusion: vmrun is a shim in a scratch
// folder and the DHCP leases are invented.
import assert from 'node:assert/strict';
import { spawn, spawnSync } from 'node:child_process';
import { chmodSync, closeSync, readdirSync, existsSync, linkSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, before, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { efiBootImages, patchNoPrompt } from './vm-win/noprompt-iso.mjs';
import { macOnly, warmDev } from './vm-test-support.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const read = (p) => readFileSync(join(here, p), 'utf8');
// Fakes live under the per-user temp root the scripts trust (getconf DARWIN_USER_TEMP_DIR on macOS, /tmp elsewhere), never $TMPDIR.
const tempRoot = (() => { const r = spawnSync('getconf', ['DARWIN_USER_TEMP_DIR'], { encoding: 'utf8' }); return r.status === 0 && r.stdout.trim() ? r.stdout.trim() : '/tmp'; })();
const scratch = mkdtempSync(join(tempRoot, 'vm-win-test-'));
const fakeVm = (dir) => { mkdirSync(dir, { recursive: true }); writeFileSync(join(dir, '.paddock-fake-vm'), 'test\n'); return dir; };
after(() => rmSync(scratch, { recursive: true, force: true }));
before(warmDev);

// The interim VM lock (scripts/vm-lock.sh) is always taken under a scratch root, never the host's real one.
const lockTmp = join(scratch, 'lock-tmp'); // owner folders of the lock's holders live here, so a holder killed by a test ends with the scratch folder
mkdirSync(lockTmp, { recursive: true });
const lockEnv = { TMPDIR: lockTmp, PADDOCK_VM_LOCK_ROOT: join(scratch, 'lock-root'), PADDOCK_VM_LOCK_HELD: '', PADDOCK_DEV: join(here, '..', 'dev') };
const run = (script, args, env = {}) =>
  spawnSync('bash', [join(here, script), ...args], { encoding: 'utf8', env: { ...process.env, ...lockEnv, ...env } });

const fakeFusion = (label) => {
  const fusion = join(scratch, `${label}Fusion.app`);
  const log = join(scratch, `${label}-calls.log`);
  mkdirSync(join(fusion, 'Contents/Library'), { recursive: true });
  writeFileSync(join(fusion, 'Contents/Library/vmrun'), `#!/bin/sh\necho "vmrun $*" >> '${log}'\necho "Total running VMs: 0"\n`);
  chmodSync(join(fusion, 'Contents/Library/vmrun'), 0o755);
  const bin = join(scratch, `${label}-bin`);
  mkdirSync(bin, { recursive: true });
  for (const tool of ['ssh', 'scp', 'security', 'hdiutil']) {
    writeFileSync(join(bin, tool), `#!/bin/sh\necho "${tool} $*" >> '${log}'\nexit 1\n`);
    chmodSync(join(bin, tool), 0o755);
  }
  return { fusion, log, path: `${bin}:${process.env.PATH}` };
};
const calls = (log) => (existsSync(log) ? readFileSync(log, 'utf8') : '');
// Tests that pass no target still get a refused folder and the logging fake Fusion, so they can never default to the real VM.
const refusedDir = '/Volumes/no-such-volume-paddock-test/vms/no-target';
const noTargetRun = (script, args, label) => {
  const f = fakeFusion(label);
  const r = run(script, args, { PADDOCK_WIN_VM_DIR: refusedDir, FUSION_APP: f.fusion, PATH: f.path });
  assert.equal(calls(f.log), '');
  return r;
};

describe('autounattend.xml.in', () => {
  const xml = read('vm-win/autounattend.xml.in');

  test('is well-formed where Setup is strict: no double hyphen inside a comment', () => {
    for (const [, body] of xml.matchAll(/<!--([\s\S]*?)-->/g)) assert.ok(!body.includes('--'), `comment contains "--": ${body.trim().slice(0, 80)}`);
  });

  test('has exactly the placeholders prepare.sh fills', () => {
    assert.deepEqual([...new Set(xml.match(/@@[A-Z_]+@@/g))].sort(), ['@@ADMIN_PASSWORD@@', '@@INPUT_LOCALE@@', '@@LANG@@', '@@TEST_PASSWORD@@', '@@TPM_BYPASS@@']);
  });

  test('carries no product key but the published generic Pro key (the VM stays unactivated)', () => {
    const keys = xml.match(/\b[A-Z0-9]{5}(?:-[A-Z0-9]{5}){4}\b/g) ?? [];
    assert.deepEqual([...new Set(keys)], ['W269N-WFGWX-YVC9B-4J6C9-T83GX']);
  });

  test('targets Arm64 and creates the setup admin and the standard test user', () => {
    const arch = [...xml.matchAll(/processorArchitecture="([^"]+)"/g)].map((m) => m[1]);
    assert.ok(arch.length > 0 && arch.every((a) => a === 'arm64'));
    assert.match(xml, /<Name>paddock-admin<\/Name>\s*<DisplayName>[^<]*<\/DisplayName>\s*<Group>Administrators<\/Group>/);
    assert.match(xml, /<Name>paddock-test<\/Name>\s*<DisplayName>[^<]*<\/DisplayName>\s*<Group>Users<\/Group>/);
  });
});

describe('firstlogon.ps1', () => {
  const ps = read('vm-win/firstlogon.ps1');

  test('makes SSH key-only and gives Git CRLF checkouts', () => {
    assert.match(ps, /^PasswordAuthentication no$/m);
    assert.match(ps, /^KbdInteractiveAuthentication no$/m);
    assert.match(ps, /core\.autocrlf true/);
  });

  test('keeps Defender on and creates no Dev Drive', () => {
    assert.doesNotMatch(ps, /Set-MpPreference|DisableRealtimeMonitoring|DevDrive/i);
    assert.match(ps, /RealTimeProtectionEnabled/);
  });

  test('never lets the guest sleep', () => {
    for (const s of ['standby-timeout-ac', 'hibernate-timeout-ac']) assert.ok(ps.includes(`'${s}'`));
    assert.match(ps, /powercfg \/hibernate off/);
  });
});

test('payload.lock pins every installer to an HTTPS URL and a SHA-256', () => {
  const lines = read('vm-win/payload.lock').split('\n').filter((l) => l && !l.startsWith('#'));
  assert.ok(lines.length >= 2);
  for (const l of lines) assert.match(l, /^\S+ https:\/\/github\.com\/\S+ [0-9a-f]{64}$/);
});

describe('guard rails', () => {
  test('vm-win.sh refuses a VM folder under $HOME', () => {
    const r = run('vm-win.sh', ['status'], { PADDOCK_WIN_VM_DIR: join(homedir(), 'vms') });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /must not be under \$HOME/);
  });

  test('prepare.sh refuses a VM folder under $HOME and unknown options', () => {
    assert.match(run('vm-win/prepare.sh', [], { PADDOCK_WIN_VM_DIR: join(homedir(), 'vms') }).stderr, /must not be under \$HOME/);
    assert.match(noTargetRun('vm-win/prepare.sh', ['--key=x'], 'BadOpt').stderr, /unknown option/);
  });

  test('prepare.sh checks the language and needs a keyboard for languages it has no default for', () => {
    assert.match(noTargetRun('vm-win/prepare.sh', ['--lang', 'english'], 'Lang1').stderr, /--lang must look like en-GB/);
    assert.match(noTargetRun('vm-win/prepare.sh', ['--lang', 'de-DE'], 'Lang2').stderr, /--input-locale is required for de-DE/);
    assert.match(noTargetRun('vm-win/prepare.sh', ['--lang', 'de-DE', '--input-locale', 'qwertz'], 'Lang3').stderr, /--input-locale must look like/);
  });

  test('prepare.sh --vm-only refuses to run without a setup disc and changes nothing else', macOnly, () => {
    const dir = fakeVm(join(scratch, 'vm-only'));
    const fusion = join(scratch, 'VmOnlyFusion.app');
    mkdirSync(join(fusion, 'Contents/Library'), { recursive: true });
    writeFileSync(join(fusion, 'Contents/Library/vmrun'), '#!/bin/sh\nexit 0\n');
    chmodSync(join(fusion, 'Contents/Library/vmrun'), 0o755);
    const r = run('vm-win/prepare.sh', ['--vm-only'], { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: fusion, PADDOCK_VM_MAX_LOAD: '100000' });
    assert.match(r.stderr, /needs an existing setup disc/);
    assert.equal(r.stdout.includes('== SSH keys'), false);
  });

  test('vm-win.sh rejects unknown commands with usage', () => {
    const r = noTargetRun('vm-win.sh', ['destroy'], 'Destroy');
    assert.equal(r.status, 2);
    assert.match(r.stderr, /vm-win\.sh start/);
  });

  test('vm-win.sh says what to do when the VM is not prepared', () => {
    const fusion = join(scratch, 'NoneFusion.app');
    mkdirSync(join(fusion, 'Contents/Library'), { recursive: true });
    writeFileSync(join(fusion, 'Contents/Library/vmrun'), '#!/bin/sh\nexit 0\n');
    chmodSync(join(fusion, 'Contents/Library/vmrun'), 0o755);
    const r = run('vm-win.sh', ['status'], { PADDOCK_WIN_VM_DIR: fakeVm(join(scratch, 'none')), FUSION_APP: fusion });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /run scripts\/vm-win\/prepare\.sh first/);
  });
});

// P1 of the 2026-11-08 review: only the test VM can ever be the target. A fake VM is allowed only with a fake Fusion AND a
// folder under $TMPDIR; every other folder is refused before vmrun, ssh, scp or any file write.
describe('only paddock-win11-test can be the target', () => {
  const commands = [['status'], ['start', '--no-wait'], ['stop'], ['ssh', 'exit 0'], ['put', 'paddock-test', '/etc/hosts', 'C:\\x'], ['gui-setup'], ['admin-setup'],
    ['snapshot', 'x'], ['revert', 'x'], ['ip'], ['install'], ['baseline']];

  test('another VM folder outside $TMPDIR is refused by every command, even with a fake Fusion, before any call', () => {
    const f = fakeFusion('Other');
    const other = '/Volumes/no-such-volume-paddock-test/vms/someone-elses-vm'; // not creatable, so a regression cannot leave a folder behind
    for (const args of commands) {
      const r = run('vm-win.sh', args, { PADDOCK_WIN_VM_DIR: other, FUSION_APP: f.fusion, PATH: f.path, PADDOCK_VM_MAX_LOAD: '100000' });
      assert.notEqual(r.status, 0, `${args[0]} was not refused`);
      assert.match(r.stderr, /only drives \/Volumes\/d7sst7\/vms\/paddock-win11-test/, `${args[0]}: ${r.stderr}`);
    }
    assert.equal(calls(f.log), '');
    assert.equal(existsSync(other), false);
  });

  test('a fake folder under $TMPDIR is refused with the real Fusion (no fake mode without a fake Fusion)', () => {
    const dir = fakeVm(join(scratch, 'real-fusion'));
    mkdirSync(join(dir, 'vm/paddock-win11-test.vmwarevm'), { recursive: true });
    writeFileSync(join(dir, 'vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx'), '.encoding = "UTF-8"\n');
    const r = run('vm-win.sh', ['status'], { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: '/Applications/VMware Fusion.app' });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /only drives/);
  });

  test('a folder under $TMPDIR that links to a folder outside it is refused', () => {
    const f = fakeFusion('Link');
    const link = join(scratch, 'link-out');
    symlinkSync('/usr', link);
    const r = run('vm-win.sh', ['status'], { PADDOCK_WIN_VM_DIR: link, FUSION_APP: f.fusion, PATH: f.path });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /only drives/);
    assert.equal(calls(f.log), '');
  });

  test('a .vmx that links out of the fake folder is refused', () => {
    const f = fakeFusion('VmxLink');
    const dir = fakeVm(join(scratch, 'vmx-link'));
    mkdirSync(join(dir, 'vm/paddock-win11-test.vmwarevm'), { recursive: true });
    symlinkSync('/etc/hosts', join(dir, 'vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx'));
    const r = run('vm-win.sh', ['snapshot', 'x'], { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: f.fusion, PATH: f.path });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /only drives/);
    assert.equal(calls(f.log), '');
  });

  test('fake mode cannot be switched on from the environment: a caller-set TMPDIR plus a Fusion whose Contents links to the real one is refused', () => {
    const fusion = join(scratch, 'LinkedFusion.app');
    mkdirSync(fusion, { recursive: true });
    symlinkSync('/Applications/VMware Fusion.app/Contents', join(fusion, 'Contents'));
    const victim = join(scratch, 'victim-root', 'vms', 'someone-elses-vm');
    mkdirSync(join(victim, 'vm/paddock-win11-test.vmwarevm'), { recursive: true });
    writeFileSync(join(victim, 'vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx'), '.encoding = "UTF-8"\n');
    writeFileSync(join(victim, '.paddock-fake-vm'), 'planted\n');
    for (const args of [['status'], ['snapshot', 'x']]) {
      const r = run('vm-win.sh', args, { PADDOCK_WIN_VM_DIR: victim, FUSION_APP: fusion, TMPDIR: join(scratch, 'victim-root') });
      assert.notEqual(r.status, 0, `${args[0]} was not refused`);
      assert.match(r.stderr, /only drives/);
    }
  });

  test('a .vmx with more than one hard link is refused', macOnly, () => {
    const f = fakeFusion('HardLink');
    const dir = fakeVm(join(scratch, 'hard-link'));
    const vmDir = join(dir, 'vm/paddock-win11-test.vmwarevm');
    mkdirSync(vmDir, { recursive: true });
    const outside = join(scratch, 'hard-link-other.vmx');
    writeFileSync(outside, 'guestOS = "x"\n');
    linkSync(outside, join(vmDir, 'paddock-win11-test.vmx'));
    const r = run('vm-win.sh', ['snapshot', 'x'], { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: f.fusion, PATH: f.path });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /hard link/);
    assert.equal(calls(f.log), '');
  });

  test('a fake vmrun script that names the installed Fusion is refused (a wrapper)', () => {
    const f = fakeFusion('Wrapper');
    const vmrun = join(f.fusion, 'Contents/Library/vmrun');
    writeFileSync(vmrun, `#!/bin/sh\nexec "/Applications/VMware Fusion.app/Contents/Public/vmrun" "$@"\n`);
    chmodSync(vmrun, 0o755);
    const r = run('vm-win.sh', ['status'], { PADDOCK_WIN_VM_DIR: fakeVm(join(scratch, 'wrapper-vm')), FUSION_APP: f.fusion, PATH: f.path });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /names the installed Fusion/);
    assert.equal(calls(f.log), '');
  });

  test('a fake vmrun that is not a script is refused', () => {
    const fusion = join(scratch, 'BinFusion.app');
    mkdirSync(join(fusion, 'Contents/Library'), { recursive: true });
    writeFileSync(join(fusion, 'Contents/Library/vmrun'), 'not a script');
    chmodSync(join(fusion, 'Contents/Library/vmrun'), 0o755);
    const r = run('vm-win.sh', ['status'], { PADDOCK_WIN_VM_DIR: fakeVm(join(scratch, 'bin-vm')), FUSION_APP: fusion });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /only drives/);
  });

  test('a fake folder without the marker is refused, and so is one that resolves into $HOME', () => {
    const f = fakeFusion('Marker');
    const bare = join(scratch, 'no-marker');
    mkdirSync(bare, { recursive: true });
    assert.match(run('vm-win.sh', ['status'], { PADDOCK_WIN_VM_DIR: bare, FUSION_APP: f.fusion, PATH: f.path }).stderr, /only drives/);
    const link = join(scratch, 'into-home');
    symlinkSync(homedir(), link);
    const r = run('vm-win.sh', ['status'], { PADDOCK_WIN_VM_DIR: link, FUSION_APP: f.fusion, PATH: f.path });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /\$HOME|only drives/);
    assert.equal(calls(f.log), '');
  });

  test('prepare.sh with a caller-set TMPDIR and a linked Fusion creates nothing', () => {
    const fusion = join(scratch, 'PrepLinkedFusion.app');
    mkdirSync(fusion, { recursive: true });
    symlinkSync('/Applications/VMware Fusion.app/Contents', join(fusion, 'Contents'));
    const root = join(scratch, 'prep-root');
    mkdirSync(root, { recursive: true });
    const target = join(root, 'other-vm');
    const r = run('vm-win/prepare.sh', ['--vm-only'], { PADDOCK_WIN_VM_DIR: target, FUSION_APP: fusion, TMPDIR: root, PADDOCK_VM_MAX_LOAD: '100000' });
    assert.notEqual(r.status, 0);
    assert.equal(existsSync(target), false);
  });

  test('prepare.sh refuses another VM folder before it creates anything', () => {
    const f = fakeFusion('Prep');
    const other = '/Volumes/no-such-volume-paddock-test/vms/someone-elses-vm'; // not creatable, so a regression cannot leave a folder behind
    const r = run('vm-win/prepare.sh', ['--vm-only'], { PADDOCK_WIN_VM_DIR: other, FUSION_APP: f.fusion, PATH: f.path, PADDOCK_VM_MAX_LOAD: '100000' });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /only drives \/Volumes\/d7sst7\/vms\/paddock-win11-test/);
    assert.equal(existsSync(other), false);
    assert.equal(calls(f.log), '');
  });
});

describe('vmrun password', () => {
  test('an unencrypted VM gets no -vp and never reads the keychain', macOnly, () => {
    const dir = fakeVm(join(scratch, 'plain'));
    const fusion = join(scratch, 'PlainFusion.app');
    const bin = join(scratch, 'plain-bin');
    const log = join(scratch, 'plain-vmrun.log');
    mkdirSync(join(dir, 'vm/paddock-win11-test.vmwarevm'), { recursive: true });
    writeFileSync(join(dir, 'vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx'), '.encoding = "UTF-8"\nguestOS = "arm-windows11-64"\n');
    mkdirSync(join(fusion, 'Contents/Library'), { recursive: true });
    writeFileSync(join(fusion, 'Contents/Library/vmrun'), `#!/bin/sh\necho "$*" >> '${log}'\necho "Total snapshots: 0"\n`);
    chmodSync(join(fusion, 'Contents/Library/vmrun'), 0o755);
    mkdirSync(bin, { recursive: true });
    writeFileSync(join(bin, 'security'), `#!/bin/sh\necho security >> '${log}'\nexit 1\n`);
    chmodSync(join(bin, 'security'), 0o755);
    const r = run('vm-win.sh', ['snapshot', 'probe'], { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: fusion, PATH: `${bin}:${process.env.PATH}` });
    assert.equal(r.status, 0, r.stderr);
    const calls = readFileSync(log, 'utf8');
    assert.match(calls, /^-T fusion snapshot \S+paddock-win11-test\.vmx probe$/m);
    assert.doesNotMatch(calls, /-vp|security/);
  });
});

describe('revert chooses the snapshot', () => {
  const setup = (label) => {
    const dir = fakeVm(join(scratch, label));
    const fusion = join(scratch, `${label}Fusion.app`);
    const log = join(scratch, `${label}-vmrun.log`);
    mkdirSync(join(dir, 'vm/paddock-win11-test.vmwarevm'), { recursive: true });
    writeFileSync(join(dir, 'vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx'), '.encoding = "UTF-8"\nguestOS = "arm-windows11-64"\n');
    mkdirSync(join(fusion, 'Contents/Library'), { recursive: true });
    writeFileSync(join(fusion, 'Contents/Library/vmrun'), `#!/bin/sh\necho "$*" >> '${log}'\nfor a in "$@"; do [ "$a" = listSnapshots ] && printf 'Total snapshots: 2\\nclean-baseline\\nadmin-baseline\\n'; done\nexit 0\n`);
    chmodSync(join(fusion, 'Contents/Library/vmrun'), 0o755);
    const revert = (args, env = {}) => run('vm-win.sh', ['revert', ...args], { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: fusion, ...env });
    return { revert, reverted: () => [...calls(log).matchAll(/revertToSnapshot \S+ (\S+)$/gm)].map((m) => m[1]) };
  };

  test('the default stays clean-baseline', macOnly, () => {
    const t = setup('rv-default');
    assert.equal(t.revert([]).status, 0);
    assert.deepEqual(t.reverted(), ['clean-baseline']);
  });

  test('--snapshot <name> and the positional name choose another one', macOnly, () => {
    const t = setup('rv-choose');
    assert.equal(t.revert(['--snapshot', 'admin-baseline']).status, 0);
    assert.equal(t.revert(['admin-baseline']).status, 0);
    assert.deepEqual(t.reverted(), ['admin-baseline', 'admin-baseline']);
  });

  test('--snapshot without a name, or an unknown snapshot, fails before any revert', macOnly, () => {
    const t = setup('rv-bad');
    const a = t.revert(['--snapshot']);
    assert.notEqual(a.status, 0);
    assert.match(a.stderr, /usage: vm-win\.sh revert --snapshot <name>/);
    const b = t.revert(['--snapshot', 'no-such']);
    assert.notEqual(b.status, 0);
    assert.match(b.stderr, /no snapshot named no-such/);
    assert.deepEqual(t.reverted(), []);
  });
});

// admin-setup and stop with a stateful fake vmrun and fake ssh/scp (review of 58ca20fb3, P1-1..P1-3): nothing touches Fusion or the real VM.
describe('admin-setup and stop: cleanup, hangs, lock', macOnly, () => {
  const VMX = 'paddock-win11-test.vmwarevm/paddock-win11-test.vmx';
  /** A fake VM dir with keys and leases, a stateful vmrun (running flag, snapshot list) and fake ssh/scp driven by env: FAKE_SSH_FAIL / FAKE_SSH_HANG (text in the command). */
  const rig = (label, { snaps = 'clean-baseline', vmxExtra = '' } = {}) => {
    const dir = fakeVm(join(scratch, `${label}-vm`));
    const vmx = join(dir, 'vm', VMX);
    mkdirSync(dirname(vmx), { recursive: true });
    writeFileSync(vmx, `.encoding = "UTF-8"\nguestOS = "arm-windows11-64"\n${vmxExtra}`);
    mkdirSync(join(dir, 'keys'), { recursive: true });
    writeFileSync(join(dir, 'keys/setup-passwords'), 'paddock-admin=a\npaddock-test=fake-pw\n');
    const leases = join(scratch, `${label}.leases`);
    writeFileSync(leases, 'lease 172.16.119.50 {\n  hardware ethernet 00:50:56:2a:d0:c7;\n}\n');
    const st = mkdtempSync(join(scratch, `${label}-state-`));
    const log = join(st, 'calls.log');
    const pids = join(st, 'hung.pids');
    writeFileSync(join(st, 'snaps'), `${snaps.split(',').join('\n')}\n`);
    const fusion = join(scratch, `${label}Fusion.app`);
    mkdirSync(join(fusion, 'Contents/Library'), { recursive: true });
    const sh = (file, body) => { writeFileSync(file, `#!/bin/sh\n${body}\n`); chmodSync(file, 0o755); };
    sh(join(fusion, 'Contents/Library/vmrun'), `echo "vmrun $*" >> '${log}'
if [ -n "$FAKE_VMRUN_HANG" ] && [ "$3" = "$FAKE_VMRUN_HANG" ]; then echo $$ >> '${pids}.vmrun'; exec tail -f /dev/null; fi
case "$3" in
  list) echo "Total running VMs: 0"; [ -f '${st}/running' ] && echo '${vmx}'; exit 0 ;;
  start) touch '${st}/running' ;;
  stop | revertToSnapshot) rm -f '${st}/running' ;;
  listSnapshots) echo "Total snapshots: x"; cat '${st}/snaps' ;;
  snapshot) echo "$5" >> '${st}/snaps' ;;
  deleteSnapshot) grep -vFx "$5" '${st}/snaps' > '${st}/snaps.new'; mv '${st}/snaps.new' '${st}/snaps' ;;
esac
exit 0`);
    const bin = join(scratch, `${label}-bin`);
    mkdirSync(bin, { recursive: true });
    sh(join(bin, 'ssh'), `for a; do last=$a; done
echo "ssh $last" >> '${log}'
case "$last" in *shutdown*) [ -n "$FAKE_SSH_NOSHUT" ] || rm -f '${st}/running' ;; esac
if [ -n "$FAKE_SSH_FAIL" ] && printf %s "$last" | grep -Fq -- "$FAKE_SSH_FAIL"; then echo "fake ssh: boom" >&2; exit 3; fi
if [ -n "$FAKE_SSH_HANG" ] && printf %s "$last" | grep -Fq -- "$FAKE_SSH_HANG"; then echo $$ >> '${pids}'; exec tail -f /dev/null; fi
exit 0`);
    sh(join(bin, 'scp'), `echo "scp $*" >> '${log}'; exit 0`);
    sh(join(bin, 'security'), 'exit 1');
    const env = { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: fusion, PATH: `${bin}:${process.env.PATH}`, PADDOCK_VM_MAX_LOAD: '100000', PADDOCK_VM_LEASES: leases, FAKE_SSH_FAIL: '', FAKE_SSH_HANG: '', FAKE_SSH_NOSHUT: '', FAKE_VMRUN_HANG: '' };
    const hungVmrun = () => (existsSync(`${pids}.vmrun`) ? readFileSync(`${pids}.vmrun`, 'utf8').split('\n').filter(Boolean).map(Number) : []);
    const hung = () => (existsSync(pids) ? readFileSync(pids, 'utf8').split('\n').filter(Boolean).map(Number) : []);
    const alive = (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } };
    return { env, log, st, vmx, hung, hungVmrun, alive, running: () => existsSync(join(st, 'running')), snaps: () => readFileSync(join(st, 'snaps'), 'utf8').split('\n').filter(Boolean), calls: () => calls(log), vmrunCalls: () => calls(log).split('\n').filter((l) => l.startsWith('vmrun ')) };
  };
  const killHung = (t) => t.hung().forEach((p) => { try { process.kill(p, 'SIGKILL'); } catch { /* gone */ } });
  const stopsAndReverts = (t) => {
    const v = t.vmrunCalls().filter((l) => / (stop|revertToSnapshot) /.test(l));
    assert.ok(v.some((l) => / stop .*hard$/.test(l)), `stop hard in ${v}`);
    assert.match(v.at(-1), /revertToSnapshot \S+ clean-baseline$/, 'the last call reverts to clean-baseline');
  };
  const neverTouchedBaseline = (t) => assert.doesNotMatch(t.calls(), /vmrun -T fusion (snapshot|deleteSnapshot) \S+ clean-baseline/);

  test('a complete admin-setup takes admin-baseline once, replaces an old one, and ends reverted and off', () => {
    const t = rig('as-ok', { snaps: 'clean-baseline,admin-baseline' });
    const r = run('vm-win.sh', ['admin-setup'], t.env);
    assert.equal(r.status, 0, r.stderr);
    assert.deepEqual(t.snaps(), ['clean-baseline', 'admin-baseline']);
    assert.match(t.calls(), /deleteSnapshot \S+ admin-baseline/);
    assert.match(t.calls(), /vmrun -T fusion snapshot \S+ admin-baseline$/m);
    assert.equal(t.running(), false);
    assert.doesNotMatch(t.calls(), / stop \S+ hard/);
    assert.match(t.vmrunCalls().at(-1), /revertToSnapshot \S+ clean-baseline$/);
    neverTouchedBaseline(t);
    assert.doesNotMatch(r.stderr, /::warning::|::error::/);
  });

  test('P1-3a: a failing PowerShell step exits 1 with ::error::, stops hard, reverts to clean-baseline, and never snapshots or deletes clean-baseline', () => {
    const t = rig('as-fail');
    const r = run('vm-win.sh', ['admin-setup'], { ...t.env, FAKE_SSH_FAIL: '-File C:\\paddock-test\\admin-setup.ps1' });
    assert.equal(r.status, 1, r.stderr);
    assert.match(r.stderr, /::error::admin-setup failed at: admin-setup\.ps1/);
    assert.match(r.stderr, /::error::admin-setup did not finish/);
    stopsAndReverts(t);
    assert.equal(t.running(), false);
    assert.doesNotMatch(t.calls(), /vmrun -T fusion (snapshot|deleteSnapshot) /);
    neverTouchedBaseline(t);
  });

  test('P1-1: a PowerShell step that hangs ends at its deadline, kills the hung ssh, and cleans up', () => {
    const t = rig('as-hang');
    const r = spawnSync('bash', [join(here, 'vm-win.sh'), 'admin-setup'], { encoding: 'utf8', timeout: 60_000, env: { ...process.env, ...lockEnv, ...t.env, FAKE_SSH_HANG: '-File C:\\paddock-test\\admin-setup.ps1', PADDOCK_VM_STEP_DEADLINE: '2' } }); // conformance: hang-guard — 60 s only stops a regression that never ends
    killHung(t);
    assert.equal(r.status, 1, `${r.status} ${r.signal} ${r.stderr}`);
    assert.match(r.stderr, /::error::no answer within 2s/);
    stopsAndReverts(t);
    assert.ok(t.hung().length > 0, 'the ssh did hang');
  });

  test('P1-1: TERM in the middle of a step stops the VM, reverts to clean-baseline and leaves no ssh behind', async () => {
    const t = rig('as-term');
    const child = spawn('bash', [join(here, 'vm-win.sh'), 'admin-setup'], { env: { ...process.env, ...lockEnv, ...t.env, FAKE_SSH_HANG: '-File C:\\paddock-test\\admin-setup.ps1' }, stdio: ['ignore', 'ignore', 'pipe'] });
    let err = '';
    child.stderr.on('data', (d) => { err += d; });
    const exited = new Promise((r) => child.on('exit', (code, sig) => r({ code, sig })));
    for (let i = 0; i < 300 && t.hung().length === 0; i++) await new Promise((r) => setTimeout(r, 100)); // conformance: hang-guard — a poll of a condition, 30 s bound that only stops a hung child
    assert.ok(t.hung().length > 0, 'the step is waiting');
    child.kill('SIGTERM');
    const { code } = await exited;
    killHung(t);
    assert.equal(code, 143, err);
    stopsAndReverts(t);
    assert.equal(t.running(), false);
    assert.match(err, /::error::admin-setup did not finish/);
    for (const p of t.hung()) assert.equal(t.alive(p), false, `fake ssh ${p} did not outlive the script`);
  });

  test('P2: a vmrun stop that hangs in the cleanup is ended at its deadline, with ::error:: and the next step, and the revert still runs', () => {
    const t = rig('as-cleanup-hang');
    const r = spawnSync('bash', [join(here, 'vm-win.sh'), 'admin-setup'], { encoding: 'utf8', timeout: 60_000, env: { ...process.env, ...lockEnv, ...t.env, FAKE_SSH_FAIL: '-File C:\\paddock-test\\admin-setup.ps1', FAKE_VMRUN_HANG: 'stop', PADDOCK_VM_CLEANUP_DEADLINE: '2' } }); // conformance: hang-guard — 60 s only stops a regression that never ends
    const hv = t.hungVmrun();
    const survivors = hv.filter(t.alive);
    hv.forEach((p) => { try { process.kill(p, 'SIGKILL'); } catch { /* gone */ } });
    assert.equal(r.status, 1, `${r.status} ${r.signal} ${r.stderr}`);
    assert.ok(hv.length > 0, 'the vmrun stop hung');
    assert.deepEqual(survivors, []);
    assert.match(r.stderr, /::error::no answer within 2s: vmrun_exec stop hard/);
    assert.match(r.stderr, /::error::could not power the VM off\. Next step/);
    assert.match(t.vmrunCalls().at(-1), /revertToSnapshot \S+ clean-baseline$/);
  });

  test('P1-3c: admin-setup is refused while another holder has the lock, and never reaches vmrun', async () => {
    const t = rig('as-lock');
    const marker = join(mkdtempSync(join(scratch, 'held-')), 'held');
    const holder = spawn('bash', ['-c', `. '${join(here, 'vm-lock.sh')}'; vm_lock_acquire other-holder; touch '${marker}'; while :; do sleep 0.2; done`], { env: { ...process.env, ...lockEnv }, stdio: 'ignore', detached: true });
    const exited = new Promise((r) => holder.on('exit', r));
    try {
      for (let i = 0; i < 150 && !existsSync(marker); i++) await new Promise((r) => setTimeout(r, 100)); // conformance: hang-guard — a poll of a condition, 15 s bound that only stops a hung child
      assert.ok(existsSync(marker));
      const r = run('vm-win.sh', ['admin-setup'], t.env);
      assert.equal(r.status, 75, r.stderr);
      assert.match(r.stderr, /held by other-holder/);
      assert.equal(t.calls(), '');
    } finally {
      try { process.kill(-holder.pid, 'SIGKILL'); } catch { /* gone */ }
      await exited;
    }
  });

  test('admin-setup never touches a running VM, and refuses a .vmx with VNC lines (before any change)', () => {
    const t = rig('as-refuse');
    writeFileSync(join(t.st, 'running'), '');
    const r = run('vm-win.sh', ['admin-setup'], t.env);
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /the VM is running/);
    assert.doesNotMatch(t.calls(), /revertToSnapshot|stop|start/);
    const v = rig('as-vnc', { vmxExtra: 'RemoteDisplay.vnc.enabled = "TRUE"\nRemoteDisplay.vnc.password = "x"\n' });
    const r2 = run('vm-win.sh', ['admin-setup'], v.env);
    assert.notEqual(r2.status, 0);
    assert.match(r2.stderr, /RemoteDisplay\.vnc/);
    assert.doesNotMatch(v.calls(), /revertToSnapshot|start/);
  });

  test('a shutdown that never powers the VM off is not snapshotted (clean shutdown), and the VM is stopped and reverted', () => {
    const t = rig('as-noshut');
    const r = spawnSync('bash', [join(here, 'vm-win.sh'), 'admin-setup'], { encoding: 'utf8', timeout: 60_000, env: { ...process.env, ...lockEnv, ...t.env, FAKE_SSH_NOSHUT: '1', PADDOCK_VM_OFF_DEADLINE: '1' } }); // conformance: hang-guard — 60 s only stops a regression
    assert.equal(r.status, 1, `${r.status} ${r.stderr}`);
    assert.match(r.stderr, /::error::admin-setup failed at: clean shutdown/);
    assert.doesNotMatch(t.calls(), /vmrun -T fusion snapshot /);
    stopsAndReverts(t);
  });

  test('P1-3b / P1-2: stop with a hanging shutdown ssh returns, and no fake ssh survives', () => {
    const t = rig('stop-hang');
    writeFileSync(join(t.st, 'running'), '');
    const r = spawnSync('bash', [join(here, 'vm-win.sh'), 'stop'], { encoding: 'utf8', timeout: 60_000, env: { ...process.env, ...lockEnv, ...t.env, FAKE_SSH_HANG: 'shutdown' } }); // conformance: hang-guard — 60 s only stops a regression that never ends
    const pids = t.hung();
    const survivors = pids.filter(t.alive);
    killHung(t);
    assert.equal(r.status, 0, `${r.status} ${r.signal} ${r.stderr}`);
    assert.match(r.stdout, /stopped/);
    assert.ok(pids.length > 0, 'the shutdown ssh ran');
    assert.deepEqual(survivors, [], 'the hung ssh was ended by stop');
  });

  test('P1-2: a failed shutdown request is reported with its exit code and stderr tail (ER-27)', () => {
    const t = rig('stop-fail');
    writeFileSync(join(t.st, 'running'), '');
    const r = run('vm-win.sh', ['stop'], { ...t.env, FAKE_SSH_FAIL: 'shutdown' });
    assert.equal(r.status, 0, r.stderr);
    assert.match(r.stderr, /::warning::shutdown request: exit 3; ssh said: .*fake ssh: boom/);
  });
});

describe('ip', () => {
  const dir = fakeVm(join(scratch, 'vm'));
  const fusion = join(scratch, 'Fusion.app');
  mkdirSync(join(dir, 'vm/paddock-win11-test.vmwarevm'), { recursive: true });
  writeFileSync(join(dir, 'vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx'), '.encoding = "UTF-8"\n');
  mkdirSync(join(fusion, 'Contents/Library'), { recursive: true });
  writeFileSync(join(fusion, 'Contents/Library/vmrun'), '#!/bin/sh\necho "Total running VMs: 0"\n');
  chmodSync(join(fusion, 'Contents/Library/vmrun'), 0o755);
  const lease = (addr, mac) => `lease ${addr} {\n\tstarts 4 2026/10/29 10:00:00;\n\thardware ethernet ${mac};\n}\n`;
  const env = (leases) => {
    const f = join(scratch, `leases-${Math.random().toString(36).slice(2)}`);
    writeFileSync(f, leases);
    return { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: fusion, PADDOCK_VM_LEASES: f };
  };

  test('takes the newest lease of the test VM fixed MAC and ignores other VMs', macOnly, () => {
    const leases = lease('192.0.2.10', '00:50:56:2a:d0:c7') + lease('192.0.2.11', '00:0c:29:aa:bb:cc') + lease('192.0.2.12', '00:50:56:2A:D0:C7');
    const r = run('vm-win.sh', ['ip'], env(leases));
    assert.equal(r.status, 0, r.stderr);
    assert.equal(r.stdout.trim(), '192.0.2.12');
  });

  test('fails with a message when the VM has no lease yet', macOnly, () => {
    const r = run('vm-win.sh', ['ip'], env(lease('192.0.2.11', '00:0c:29:aa:bb:cc')));
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /no DHCP lease/);
  });
});

describe('noprompt-iso', () => {
  const S = 2048;
  const prompt = Buffer.alloc(3 * S, 0x11);
  const noprompt = Buffer.alloc(3 * S, 0x22);
  // A synthetic bootable image: El Torito record at sector 17, catalog at 19, the EFI image at 24, a UDF-like marker at 30.
  const image = ({ defaultPlatform = 0xef, efiSection = false, boot = prompt } = {}) => {
    const b = Buffer.alloc(40 * S);
    const brvd = b.subarray(17 * S);
    brvd[0] = 0;
    brvd.write('CD001', 1, 'latin1');
    brvd[6] = 1;
    brvd.write('EL TORITO SPECIFICATION', 7, 'latin1');
    brvd.writeUInt32LE(19, 0x47);
    const cat = b.subarray(19 * S);
    cat[0] = 1;
    cat[1] = defaultPlatform;
    cat[30] = 0x55;
    cat[31] = 0xaa;
    cat[32] = 0x88;
    cat.writeUInt32LE(efiSection ? 22 : 24, 32 + 8); // a BIOS image at 22 when EFI is in a section
    if (efiSection) {
      cat[64] = 0x91;
      cat[65] = 0xef;
      cat.writeUInt16LE(1, 66);
      cat[96] = 0x88;
      cat.writeUInt32LE(24, 96 + 8);
    }
    boot.copy(b, 24 * S);
    b.write('UDF-TREE', 30 * S, 'latin1');
    return b;
  };
  const withFile = (buf, fn) => {
    const f = join(scratch, `iso-${Math.random().toString(36).slice(2)}.iso`);
    writeFileSync(f, buf);
    const fd = openSync(f, 'r+');
    try {
      return { result: fn(fd), after: readFileSync(f) };
    } finally {
      closeSync(fd);
    }
  };

  test('finds the EFI image as the default entry (Arm64 media) or in an EFI section (x64 media)', () => {
    for (const opts of [{}, { defaultPlatform: 0, efiSection: true }]) {
      const b = image(opts);
      assert.deepEqual(efiBootImages((o, l) => b.subarray(o, o + l)), [24]);
    }
  });

  test('replaces only the boot image bytes, then reports already on a second run', () => {
    const before = image();
    const { result, after } = withFile(before, (fd) => patchNoPrompt(fd, prompt, noprompt));
    assert.equal(result, 'patched');
    assert.ok(after.subarray(24 * S, 27 * S).equals(noprompt));
    assert.ok(after.subarray(0, 24 * S).equals(before.subarray(0, 24 * S)));
    assert.ok(after.subarray(27 * S).equals(before.subarray(27 * S)));
    assert.equal(withFile(after, (fd) => patchNoPrompt(fd, prompt, noprompt)).result, 'already');
  });

  test('refuses an unknown boot image, different sizes, and a disc that is not bootable', () => {
    assert.throws(() => withFile(image({ boot: Buffer.alloc(3 * S, 0x33) }), (fd) => patchNoPrompt(fd, prompt, noprompt)), /neither efisys/);
    assert.throws(() => withFile(image(), (fd) => patchNoPrompt(fd, prompt, Buffer.alloc(S))), /differ in size/);
    assert.throws(() => withFile(Buffer.alloc(40 * S), (fd) => patchNoPrompt(fd, prompt, noprompt)), /not a bootable ISO/);
  });
});

// The interim exclusive lock (scripts/vm-lock.sh): two users of the test VM at once loaded every vCPU and a power-off cut the other
// off (2026-11-08). Every command that changes or loads the guest takes it; status, ip and help stay free.
describe('the VM lock', () => {
  const fakeUsable = (label) => {
    const f = fakeFusion(label);
    const dir = fakeVm(join(scratch, `${label}-vm`));
    mkdirSync(join(dir, 'vm/paddock-win11-test.vmwarevm'), { recursive: true });
    writeFileSync(join(dir, 'vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx'), '.encoding = "UTF-8"\n');
    return { ...f, env: { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: f.fusion, PATH: f.path, PADDOCK_VM_MAX_LOAD: '100000' } };
  };
  // The holder lives until the test kills its process group: no fixed lifetime (ER-12), a loop on a condition that never comes.
  const holding = async (label, body = 'while :; do sleep 0.2; done') => {
    // One marker per call: a label used twice must not return on the first call's leftover marker.
    const marker = join(mkdtempSync(join(scratch, 'held-')), 'held');
    const child = spawn('bash', ['-c', `. '${join(here, 'vm-lock.sh')}'; vm_lock_acquire ${label}; touch '${marker}'; ${body}`], { env: { ...process.env, ...lockEnv }, stdio: 'ignore', detached: true });
    const exited = new Promise((r) => child.on('exit', r));
    // A poll of a condition: the bound only stops a hung child, the verdict never depends on the time taken.
    for (let i = 0; i < 150 && !existsSync(marker); i++) await new Promise((r) => setTimeout(r, 100)); // conformance: hang-guard — a poll of a condition, 15 s bound that only stops a hung child
    assert.ok(existsSync(marker), `${label} took the lock`);
    return { stop: async () => { try { process.kill(-child.pid, 'SIGKILL'); } catch { /* gone */ } await exited; } };
  };

  test('holding() returns only once its own holder has the lock, also when an earlier call used the same label', async () => {
    const owner = () => readFileSync(join(lockEnv.PADDOCK_VM_LOCK_ROOT, 'slots', 's1.owner'), 'utf8').match(/owner=(.*)/)?.[1];
    const first = await holding('same-label');
    const firstOwner = owner();
    assert.ok(firstOwner, 'the first holder holds the lock');
    await first.stop();
    const second = await holding('same-label');
    try {
      assert.notEqual(owner(), firstOwner, 'the slot names the second holder, not the killed first one (a marker left by the first call must not count)');
    } finally {
      await second.stop();
    }
  });

  test('a second vm-win.sh start fails while chain-release.sh holds the lock, names the holder, and never reaches vmrun', macOnly, async () => {
    const f = fakeUsable('Locked');
    const holder = await holding('chain-release');
    try {
      const r = run('vm-win.sh', ['start', '--no-wait'], f.env);
      assert.notEqual(r.status, 0);
      assert.match(r.stderr, /::error::.*held by chain-release \(pid \d+, started /);
      assert.equal(calls(f.log), '', 'no vmrun call');
      const status = run('vm-win.sh', ['status'], f.env);
      assert.equal(status.status, 0, 'status is read-only and not locked');
    } finally {
      await holder.stop();
    }
  });

  test('a command run by the holder inherits the lock (PADDOCK_VM_LOCK_HELD) and reaches vmrun; the lock is free afterwards', macOnly, async () => {
    const f = fakeUsable('Inherit');
    const inner = `bash '${join(here, 'vm-win.sh')}' start --no-wait`;
    const holder = spawn('bash', ['-c', `. '${join(here, 'vm-lock.sh')}'; vm_lock_acquire chain-release; ${inner}`], { env: { ...process.env, ...lockEnv, ...f.env }, stdio: 'ignore' });
    await new Promise((r) => holder.on('exit', r));
    assert.match(calls(f.log), /vmrun -T fusion start .* nogui/);
    const again = run('vm-win.sh', ['start', '--no-wait'], f.env);
    assert.equal(again.status, 0, again.stderr);
  });

  test('run-win.mjs --help prints usage and exits 0 without the lock, even while another run holds it', async () => {
    const holder = await holding('chain-release');
    try {
      const r = spawnSync('node', [join(here, 'vm-smoke/run-win.mjs'), '--help'], { encoding: 'utf8', env: { ...process.env, ...lockEnv }, timeout: 60_000 });
      assert.equal(r.status, 0, r.stderr);
      assert.match(r.stdout, /run-win\.mjs --app dyno/);
      assert.doesNotMatch(r.stderr, /vm-lock|::error::/);
    } finally {
      await holder.stop();
    }
  });

  test('run-win.mjs and defender-rate.mjs take the lock for the whole run: a second one fails at the start (75), before any vm-win call', async () => {
    const holder = await holding('chain-release');
    try {
      for (const [file, args] of [['vm-smoke/run-win.mjs', ['--app', 'dyno', '--installer', '/nonexistent/setup.exe']], ['vm-smoke/defender-rate.mjs', ['--installer', '/nonexistent/setup.exe']]]) {
        const r = spawnSync('node', [join(here, file), ...args], { encoding: 'utf8', env: { ...process.env, ...lockEnv }, timeout: 60_000 });
        assert.equal(r.status, 75, `${file}: ${r.stdout}${r.stderr}`);
        assert.match(r.stderr, /::error::.*held by chain-release/);
        assert.doesNotMatch(r.stdout + r.stderr, /vm-win|revert|snapshot/i, `${file} did nothing before failing`);
      }
    } finally {
      await holder.stop();
    }
  });

  test('make-no-dyno-snapshot.sh and chain-release.sh take the lock; no runner clears the environment it passes to vm-win.sh', () => {
    for (const f of ['vm-smoke/make-no-dyno-snapshot.sh', 'vm-smoke/chain-release.sh']) assert.match(read(f), /vm_lock_acquire /, f);
    assert.doesNotMatch(read('vm-smoke/chain-release.sh'), /\$PLOCK|plock run/);
    for (const f of readdirSync(join(here, 'vm-smoke')).filter((n) => /\.mjs$/.test(n) && !n.endsWith('.test.mjs'))) {
      const text = read(`vm-smoke/${f}`);
      if (!/vm-win\.sh|VMWIN|vmwin/.test(text)) continue;
      for (const m of text.matchAll(/[ ,{]env:\s*([^,}]+)/g)) assert.match(m[1].trim(), /^(process\.env|\{ \.\.\.process\.env|env \?\? process\.env|tartEnv)/, `${f}: ${m[0]}`);
    }
  });
});
