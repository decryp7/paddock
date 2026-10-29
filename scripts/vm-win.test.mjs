// The local Windows test VM tooling (docs/testing/windows-test-vm.md): the answer file, the pins and the guard rails of
// scripts/vm-win.sh and scripts/vm-win/prepare.sh. Nothing here starts a VM or touches Fusion: vmrun is a shim in a scratch
// folder and the DHCP leases are invented.
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { homedir, tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { after, describe, test } from 'node:test';
import { fileURLToPath } from 'node:url';

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

describe('vmrun password', () => {
  test('an unencrypted VM gets no -vp and never reads the keychain', () => {
    const dir = join(scratch, 'plain');
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
