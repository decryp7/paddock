// Tests for check-agent-version.sh: a fake agent prints the same multi-line report as paddock_proc::self_test.
import test from "node:test";
import assert from "node:assert/strict";
import { chmodSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const script = new URL("./check-agent-version.sh", import.meta.url).pathname;
const dir = mkdtempSync(join(tmpdir(), "agent-version-"));

function fakeAgent(name, body) {
  const path = join(dir, name);
  writeFileSync(path, `#!/bin/sh\n${body}\n`);
  chmodSync(path, 0o755);
  return path;
}
const run = (agent, version) => spawnSync("bash", [script, agent, version], { encoding: "utf8" });

const report = "echo 'shell-demo-agent 0.1.1 self-test'; echo 'ok executable'; echo 'ok service-spec'";

test("a multi-line report with the expected version passes", () => {
  const r = run(fakeAgent("good", report), "0.1.1");
  assert.equal(r.status, 0, r.stdout + r.stderr);
});

test("a different version fails and prints the report", () => {
  const r = run(fakeAgent("old", report), "0.1.2");
  assert.equal(r.status, 1);
  assert.match(r.stdout, /reports version '0\.1\.1', expected '0\.1\.2'/);
  assert.match(r.stdout, /ok service-spec/);
});

test("a failing self-test fails even with the right version", () => {
  const r = run(fakeAgent("failing", `${report}; echo 'FAIL x: y'; exit 1`), "0.1.1");
  assert.equal(r.status, 1);
  assert.match(r.stdout, /exited 1/);
  assert.match(r.stdout, /FAIL x: y/);
});

test("a missing file is named", () => {
  const r = run(join(dir, "nope"), "0.1.1");
  assert.equal(r.status, 1);
  assert.match(r.stdout, /not an executable file: .*nope/);
});
