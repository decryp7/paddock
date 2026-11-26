import assert from 'node:assert/strict';
import { test } from 'node:test';
import check, { localVmSites } from '../src/checks/C172-no-local-vm.mjs';

const rs = (t, f = 'tools/paddock-dev/src/pre_push/gates.rs') => localVmSites(t, f);

test('C172: a container or VM tool in local gate code is flagged (positive control)', () => {
  assert.deepEqual(rs('fn a() {\n    Command::new("docker");\n}\n'), [2]);
  assert.deepEqual(rs('let g = gate(id, "", "bash", &["tools/linux-container.sh", "--id"]);\n'), [1]);
  assert.deepEqual(rs('let p = "/usr/local/bin/orbctl start";\n'), [1]);
  assert.deepEqual(rs('let p = "vmrun";\n'), [1]);
  assert.deepEqual(localVmSites('set -e\ndocker run --rm x\n', 'tools/githooks/pre-push'), [2]);
  assert.deepEqual(localVmSites('x; sudo docker ps\n', 'tools/githooks/pre-push'), [1]);
});

test('C172: comments, tests, waivers, CI-only code and look-alike words are not flagged', () => {
  assert.deepEqual(rs('// docker is gone\n/// "docker"\n'), []);
  assert.deepEqual(rs('fn a() {}\n#[cfg(test)]\nmod t { fn x() { f("docker"); } }\n'), []);
  assert.deepEqual(rs('f("docker");\n', 'tools/paddock-dev/src/pre_push/tests.rs'), []);
  assert.deepEqual(rs('// conformance: no-local-vm — a CI-only helper\nf("docker");\n'), []);
  assert.deepEqual(rs('f("docker");\n', 'tools/paddock-dev/src/gates/ci.rs'), [], 'CI-only code is out of scope');
  assert.deepEqual(rs('f("dockerfile");\nf("orbit");\n'), []);
  assert.deepEqual(localVmSites('echo docker\n# docker\n', 'tools/githooks/pre-push'), [], 'an argument and a comment are not a command');
});

test('C172: the check passes its own positive control and reports warnings only', () => {
  const files = ['tools/paddock-dev/src/pre_push/gates.rs'];
  const ctx = { files: () => files, read: () => 'f("docker");\n' };
  const out = check.run(ctx);
  assert.equal(out.length, 1);
  assert.equal(out[0].severity, 'warn');
  assert.equal(check.mode, 'report-only');
});

test('C172: developer commands and shared crates are in scope, CI-only dev code is not', () => {
  const t = 'let c = "tools/linux-container.sh --id paddock";\n';
  assert.deepEqual(localVmSites(t, 'crates/paddock-testkit/src/golden.rs'), [1]);
  assert.deepEqual(localVmSites(t, 'tools/paddock-dev/src/golden_cmd.rs'), [1]);
  assert.deepEqual(localVmSites(t, 'tools/paddock-dev/src/gates/ci.rs'), []);
  assert.deepEqual(localVmSites(t, 'tools/paddock-dev/src/ci_run/x.rs'), []);
});

test('C172: app developer scripts are in scope', () => {
  assert.deepEqual(localVmSites('set -e\n"$docker" run x\ndocker run --rm x\n', 'apps/dyno/scripts/linux-verify.sh'), [3]);
});
