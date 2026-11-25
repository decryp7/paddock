// tools/mac-ci-signing.sh (the Mac CI runner's stable signing identity). `check` runs against the real codesign: an Apple-signed
// program with its own leaf certificate is the positive control, an ad-hoc copy the negative one, so the check is shown to see
// the signature TCC stores (ER-33). Creating and signing never touch the machine's keychains: security, openssl and codesign are
// shims there, and the keychain and password paths point into a scratch folder.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { copyFileSync, existsSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { calls, run, scratch, shim, write } from './helpers.mjs';

const skip = process.platform === 'darwin' ? false : 'needs macOS codesign';
const NAME = 'Paddock CI code signing';
const SHA = 'AB'.repeat(20);

/** The SHA-1 of /usr/bin/true's leaf certificate, read with codesign itself. */
function appleLeaf() {
  const dir = scratch('paddock-signing-cert-');
  const r = spawnSync('codesign', ['-d', `--extract-certificates=${join(dir, 'c')}`, '/usr/bin/true'], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  return createHash('sha1').update(readFileSync(join(dir, 'c0'))).digest('hex');
}

const signing = (args, { bin, home, env = {} } = {}) =>
  run('mac-ci-signing.sh', args, {
    bin,
    cwd: home,
    env: {
      PADDOCK_TOOLS_OS: 'Darwin',
      PADDOCK_CI_SIGNING_KEYCHAIN: join(home, 'kc', 'ci.keychain-db'),
      PADDOCK_CI_SIGNING_PASS_FILE: join(home, 'cfg', 'ci.pass'),
      ...env,
    },
  });

/** A scratch Mac whose `security` knows the identity once `imported` exists, and whose codesign accepts what it signed. */
function mac({ identity = true } = {}) {
  const home = scratch('paddock-signing-home-');
  const bin = scratch('paddock-signing-bin-');
  const imported = join(bin, 'imported');
  if (identity) write(home, { 'kc/ci.keychain-db': 'keychain', 'cfg/ci.pass': 'invented-keychain-password' });
  if (identity) write(bin, { imported: '' });
  shim(bin, 'security', `
case "$1" in
  find-certificate) [ -f "${imported}" ] && printf 'SHA-256 hash: 00\\nSHA-1 hash: ${SHA}\\n' ;;
  create-keychain) mkdir -p "$(dirname "$4")" && : > "$4" ;;
  import) : > "${imported}" ;;
  list-keychains) [ "$3" = -s ] || echo '    "/Users/someone/Library/Keychains/login.keychain-db"' ;;
esac
exit 0`);
  shim(bin, 'codesign', `
for a in "$@"; do last="$a"; done
case "$1" in
  --verify) [ -f "$last/.signed" ] || grep -q PADDOCK-SIGNED "$last" 2>/dev/null; exit $? ;;
  -d) echo 'designated => cdhash H"00"' >&2 ;;
  --force) if [ -d "$last" ]; then : > "$last/.signed"; else echo PADDOCK-SIGNED >> "$last"; fi ;;
esac`);
  shim(bin, 'openssl', 'for a in "$@"; do case "$prev" in -out|-keyout) : > "$a" ;; esac; prev="$a"; done');
  return { home, bin };
}

test('check: a program signed by the leaf passes (positive control)', { skip }, () => {
  const home = scratch('paddock-signing-home-');
  const r = signing(['check', '--identifier', 'com.apple.true', '--leaf-sha1', appleLeaf(), '/usr/bin/true'], { home });
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /ok {4}\/usr\/bin\/true: identifier "com\.apple\.true" and certificate leaf = H"/);
});

test('check: an ad-hoc copy and a wrong leaf fail with 5 and name the signature they have', { skip }, () => {
  const home = scratch('paddock-signing-home-');
  const copy = join(home, 'true');
  copyFileSync('/usr/bin/true', copy);
  assert.equal(spawnSync('codesign', ['--force', '--sign', '-', copy]).status, 0);
  const adhoc = signing(['check', '--identifier', 'com.apple.true', '--leaf-sha1', appleLeaf(), copy], { home });
  assert.equal(adhoc.status, 5, adhoc.out);
  assert.match(adhoc.out, /has: cdhash H"/);
  const wrong = signing(['check', '--identifier', 'com.apple.true', '--leaf-sha1', '0'.repeat(40), '/usr/bin/true'], { home });
  assert.equal(wrong.status, 5, wrong.out);
});

test('check without an identity is not measured (6), never a pass (ER-33)', { skip }, () => {
  const home = scratch('paddock-signing-home-');
  const r = signing(['check', '--identifier', 'x', '/usr/bin/true'], { home });
  assert.equal(r.status, 6, r.out);
  assert.match(r.out, /not measured/);
});

test('sign --if-configured without an identity leaves the program and exits 0 with a notice', { skip }, () => {
  const m = mac({ identity: false });
  write(m.home, { prog: '#!/bin/bash\n' });
  const r = signing(['sign', '--if-configured', '--identifier', 'x', join(m.home, 'prog')], m);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /::notice::mac-ci-signing: no identity/);
  assert.equal(readFileSync(join(m.home, 'prog'), 'utf8'), '#!/bin/bash\n');
  assert.ok(!calls(m.bin).some((c) => c.startsWith('codesign --force')));
});

test('sign without an identity fails with the create command', { skip }, () => {
  const m = mac({ identity: false });
  write(m.home, { prog: '#!/bin/bash\n' });
  const r = signing(['sign', '--identifier', 'x', join(m.home, 'prog')], m);
  assert.equal(r.status, 1, r.out);
  assert.match(r.out, /next: run: bash .*mac-ci-signing\.sh create/);
});

test('sign: a plain program is signed as a copy renamed over it, with the identifier, then checked; again is a no-op', { skip }, () => {
  const m = mac();
  const prog = join(m.home, 'gitea-runner');
  write(m.home, { 'gitea-runner': '#!/bin/bash\n' });
  const inode = statSync(prog).ino;
  const r = signing(['sign', '--identifier', 'net.decryptology.gitea-runner', prog], m);
  assert.equal(r.status, 0, r.out);
  const sign = calls(m.bin).find((c) => c.startsWith('codesign --force'));
  assert.match(sign, new RegExp(`--identifier net\\.decryptology\\.gitea-runner --sign ${SHA} --keychain .*ci\\.keychain-db .*gitea-runner\\.paddock-signing\\.\\d+$`));
  assert.notEqual(statSync(prog).ino, inode, 'renamed over, so a running copy keeps its pages');
  assert.ok(calls(m.bin).some((c) => c.startsWith('security unlock-keychain')));
  assert.ok(!r.out.includes('invented-keychain-password'), 'the password is never printed');
  const again = signing(['sign', '--identifier', 'net.decryptology.gitea-runner', prog], m);
  assert.equal(again.status, 0, again.out);
  assert.match(again.out, /already signed/);
  assert.equal(calls(m.bin).filter((c) => c.startsWith('codesign --force')).length, 1);
});

test('sign-playwright signs Chromium as a bundle and the headless shell with a fixed identifier', { skip }, () => {
  const m = mac();
  const dir = join(m.home, 'browsers');
  write(dir, {
    'chromium-1248/chrome-mac-arm64/Google Chrome for Testing.app/Contents/MacOS/Google Chrome for Testing': '#!/bin/bash\n',
    'chromium-1248/chrome-mac-arm64/Google Chrome for Testing.app/Contents/Info.plist':
      '<?xml version="1.0" encoding="UTF-8"?>\n<plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.google.chrome.for.testing</string></dict></plist>\n',
    'chromium_headless_shell-1248/chrome-headless-shell-mac-arm64/chrome-headless-shell': '#!/bin/bash\n',
  });
  const r = signing(['sign-playwright', dir], m);
  assert.equal(r.status, 0, r.out);
  const signs = calls(m.bin).filter((c) => c.startsWith('codesign --force'));
  assert.equal(signs.length, 2, signs.join('\n'));
  assert.ok(signs.some((c) => c.includes('--deep') && c.endsWith('Google Chrome for Testing.app')));
  assert.ok(signs.some((c) => c.includes('--identifier net.decryptology.paddock.ci.chrome-headless-shell')));
});

test('create makes the keychain and identity once, never prints a password, and a second run does nothing', { skip }, () => {
  const m = mac({ identity: false });
  const r = signing(['create'], m);
  assert.equal(r.status, 0, r.out);
  const pass = readFileSync(join(m.home, 'cfg', 'ci.pass'), 'utf8');
  assert.match(pass, /^[A-Za-z0-9]{40}$/);
  assert.equal(statSync(join(m.home, 'cfg', 'ci.pass')).mode & 0o777, 0o600);
  assert.ok(!r.out.includes(pass), 'the keychain password is masked');
  assert.ok(!calls(m.bin).join('\n').includes('pass:') || r.out.includes('pass:***'));
  for (const s of ['security create-keychain', 'security import', 'security set-key-partition-list -S apple-tool:,apple:,codesign:', 'security list-keychains -d user -s'])
    assert.ok(calls(m.bin).some((c) => c.startsWith(s)), `missing: ${s}`);
  assert.ok(calls(m.bin).some((c) => c.startsWith('security import') && c.includes('-T /usr/bin/codesign')));
  assert.match(r.out, new RegExp(`ok: identity '${NAME}', SHA-1 ${SHA}`));
  const again = signing(['create'], m);
  assert.equal(again.status, 0, again.out);
  assert.match(again.out, /exists .*nothing to do/);
  assert.ok(existsSync(join(m.home, 'kc', 'ci.keychain-db')));
});

test('create --dry-run writes nothing', { skip }, () => {
  const m = mac({ identity: false });
  const r = signing(['create', '--dry-run'], m);
  assert.equal(r.status, 0, r.out);
  assert.match(r.out, /Dry run: nothing was changed/);
  assert.ok(!existsSync(join(m.home, 'cfg', 'ci.pass')));
  assert.ok(!calls(m.bin).some((c) => /^security (create-keychain|import)/.test(c)));
});
