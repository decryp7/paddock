// The local Windows test VM tooling (docs/testing/windows-test-vm.md): the answer file, the pins and the guard rails of
// scripts/vm-win.sh and scripts/vm-win/prepare.sh. Nothing here starts a VM or touches Fusion: vmrun is a shim in a scratch
// folder and the DHCP leases are invented.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, closeSync, mkdirSync, mkdtempSync, openSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';
import { efiBootImages, patchNoPrompt } from './vm-win/noprompt-iso.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const read = (p) => readFileSync(join(here, p), 'utf8');
const scratch = mkdtempSync(join(tmpdir(), 'vm-win-test-'));
after(() => rmSync(scratch, { recursive: true, force: true }));

const run = (script, args, env = {}) =>
  spawnSync('bash', [join(here, script), ...args], { encoding: 'utf8', env: { ...process.env, ...env } });

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
    assert.match(run('vm-win/prepare.sh', ['--key=x']).stderr, /unknown option/);
  });

  test('prepare.sh checks the language and needs a keyboard for languages it has no default for', () => {
    assert.match(run('vm-win/prepare.sh', ['--lang', 'english']).stderr, /--lang must look like en-GB/);
    assert.match(run('vm-win/prepare.sh', ['--lang', 'de-DE']).stderr, /--input-locale is required for de-DE/);
    assert.match(run('vm-win/prepare.sh', ['--lang', 'de-DE', '--input-locale', 'qwertz']).stderr, /--input-locale must look like/);
  });

  test('prepare.sh --vm-only refuses to run without a setup disc and changes nothing else', () => {
    const dir = join(scratch, 'vm-only');
    const fusion = join(scratch, 'VmOnlyFusion.app');
    mkdirSync(join(fusion, 'Contents/Library'), { recursive: true });
    const r = run('vm-win/prepare.sh', ['--vm-only'], { PADDOCK_WIN_VM_DIR: dir, FUSION_APP: fusion, PADDOCK_VM_MAX_LOAD: '100000' });
    assert.match(r.stderr, /needs an existing setup disc/);
    assert.equal(r.stdout.includes('== SSH keys'), false);
  });

  test('vm-win.sh rejects unknown commands with usage', () => {
    const r = run('vm-win.sh', ['destroy']);
    assert.equal(r.status, 2);
    assert.match(r.stderr, /vm-win\.sh start/);
  });

  test('vm-win.sh says what to do when the VM is not prepared', () => {
    const r = run('vm-win.sh', ['status'], { PADDOCK_WIN_VM_DIR: join(scratch, 'none') });
    assert.notEqual(r.status, 0);
    assert.match(r.stderr, /run scripts\/vm-win\/prepare\.sh first/);
  });
});

describe('the broker', () => {
  // A stand-in for `./dev`: records every call, answers `lease` with an id, `status` with a state file and `snapshots` with a list.
  const rig = (state = 'stopped') => {
    const dir = join(scratch, `broker-${Math.random().toString(36).slice(2)}`);
    mkdirSync(join(dir, 'vm/paddock-win11-test.vmwarevm'), { recursive: true });
    writeFileSync(join(dir, 'vm/paddock-win11-test.vmwarevm/paddock-win11-test.vmx'), '.encoding = "UTF-8"\n');
    const log = join(dir, 'dev.log');
    const dev = join(dir, 'dev');
    writeFileSync(dev, `#!/bin/sh\necho "$*" >> '${log}'\ncase "$2" in\n  lease) echo paddock-win11-test.1.2;;\n  status) echo ${state};;\n  snapshots) printf 'clean-baseline\\ngui-session-tools\\n';;\n  *) echo ok;;\nesac\n`);
    chmodSync(dev, 0o755);
    return { dir, log, env: { PADDOCK_WIN_VM_DIR: dir, PADDOCK_DEV: dev, HOME: process.env.HOME } };
  };
  const calls = (log) => readFileSync(log, 'utf8').trim().split('\n');

  test('snapshot takes a lease for this script, snapshots through the broker, and releases', () => {
    const r = rig();
    const out = run('vm-win.sh', ['snapshot', 'probe'], r.env);
    assert.equal(out.status, 0, out.stderr);
    const c = calls(r.log);
    assert.match(c[0], new RegExp(`^vm lease paddock-win11-test --for vm-win\\.sh snapshot --holder-pid \\d+$`));
    assert.equal(c[1], 'vm snapshot paddock-win11-test.1.2 --name probe');
    assert.equal(c.at(-1), 'vm release paddock-win11-test.1.2');
  });

  test('revert checks the snapshot through the broker first and refuses an unknown one without reverting', () => {
    const r = rig();
    assert.equal(run('vm-win.sh', ['revert', 'gui-session-tools'], r.env).status, 0);
    assert.ok(calls(r.log).includes('vm revert paddock-win11-test.1.2 --to gui-session-tools'));
    const r2 = rig();
    const bad = run('vm-win.sh', ['revert', 'nope'], r2.env);
    assert.notEqual(bad.status, 0);
    assert.match(bad.stderr, /no snapshot named nope/);
    assert.ok(!calls(r2.log).some((l) => l.startsWith('vm revert')));
  });

  test('stop on a stopped VM does nothing; on a running one it adopts the lease and stops through the broker', () => {
    const idle = rig('stopped');
    assert.match(run('vm-win.sh', ['stop'], idle.env).stdout, /already stopped/);
    assert.ok(!calls(idle.log).some((l) => l.includes('lease')));
    const up = rig('running');
    assert.equal(run('vm-win.sh', ['stop'], up.env).status, 0);
    const c = calls(up.log);
    assert.match(c.find((l) => l.includes('lease')), /--adopt$/);
    assert.ok(c.includes('vm stop paddock-win11-test.1.2 --grace 120'));
  });

  test('a refused lease stops the command before any VM action', () => {
    const r = rig();
    const dev = join(r.dir, 'dev');
    writeFileSync(dev, `#!/bin/sh\necho "$*" >> '${r.log}'\necho 'the VM is leased to "dyno lifecycle"' >&2\nexit 4\n`);
    const out = run('vm-win.sh', ['snapshot', 'probe'], r.env);
    assert.notEqual(out.status, 0);
    assert.match(out.stderr, /leased to "dyno lifecycle"/);
    assert.equal(calls(r.log).length, 1);
  });

  test('vm-win.sh calls no VM tool itself', () => {
    const text = read('vm-win.sh').split('\n').filter((l) => !l.trim().startsWith('#')).join('\n');
    assert.doesNotMatch(text, /vmrun|vmcli|vmware-vmx/);
  });
});

describe('ip', () => {
  const dir = join(scratch, 'vm');
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
