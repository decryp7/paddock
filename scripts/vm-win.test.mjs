// The local Windows test VM tooling (docs/testing/windows-test-vm.md): the answer file, the pins and the guard rails of
// scripts/vm-win.sh and scripts/vm-win/prepare.sh. Nothing here starts a VM or touches Fusion: vmrun is a shim in a scratch
// folder and the DHCP leases are invented.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, closeSync, existsSync, linkSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { efiBootImages, patchNoPrompt } from './vm-win/noprompt-iso.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const read = (p) => readFileSync(join(here, p), 'utf8');
// Fakes live under the per-user temp root the scripts trust (getconf DARWIN_USER_TEMP_DIR on macOS, /tmp elsewhere), never $TMPDIR.
const tempRoot = (() => { const r = spawnSync('getconf', ['DARWIN_USER_TEMP_DIR'], { encoding: 'utf8' }); return r.status === 0 && r.stdout.trim() ? r.stdout.trim() : '/tmp'; })();
const scratch = mkdtempSync(join(tempRoot, 'vm-win-test-'));
const fakeVm = (dir) => { mkdirSync(dir, { recursive: true }); writeFileSync(join(dir, '.paddock-fake-vm'), 'test\n'); return dir; };
after(() => rmSync(scratch, { recursive: true, force: true }));

const run = (script, args, env = {}) =>
  spawnSync('bash', [join(here, script), ...args], { encoding: 'utf8', env: { ...process.env, ...env } });

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

  test('prepare.sh --vm-only refuses to run without a setup disc and changes nothing else', () => {
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
  const commands = [['status'], ['start', '--no-wait'], ['stop'], ['ssh', 'exit 0'], ['put', 'paddock-test', '/etc/hosts', 'C:\\x'], ['gui-setup'],
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

  test('a .vmx with more than one hard link is refused', () => {
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
  test('an unencrypted VM gets no -vp and never reads the keychain', () => {
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

  test('takes the newest lease of the test VM fixed MAC and ignores other VMs', () => {
    const leases = lease('192.0.2.10', '00:50:56:2a:d0:c7') + lease('192.0.2.11', '00:0c:29:aa:bb:cc') + lease('192.0.2.12', '00:50:56:2A:D0:C7');
    const r = run('vm-win.sh', ['ip'], env(leases));
    assert.equal(r.status, 0, r.stderr);
    assert.equal(r.stdout.trim(), '192.0.2.12');
  });

  test('fails with a message when the VM has no lease yet', () => {
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
