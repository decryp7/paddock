// Test for the N+1 agent-version check inside build-linux-arm64.sh's container script: the agent's --self-test prints a
// multi-line report (header `<name> <version> self-test`, then `ok <check>` lines), so the check must read the header only.
// Run 7482's job 28297 failed with "N+1's agent is not 0.1.1" because awk '{print $2}' ran over every line of the report.
import test from "node:test";
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const repo = new URL("../..", import.meta.url).pathname.replace(/\/$/, "");
const heredoc = readFileSync(new URL("./build-linux-arm64.sh", import.meta.url), "utf8").split("<<'IN'\n")[1].split("\nIN\n")[0];
const checkLine = heredoc.split("\n").find((l) => l.includes("n1/paddock-shell-demo/shell-demo-agent") && !l.startsWith("#") && !l.includes("cp "));

function run(version, wanted) {
  const root = mkdtempSync(join(tmpdir(), "linux-build-"));
  const dir = join(root, "out/n1/paddock-shell-demo");
  mkdirSync(dir, { recursive: true });
  const agent = join(dir, "shell-demo-agent");
  writeFileSync(agent, `#!/bin/sh\necho 'shell-demo-agent ${version} self-test'; echo 'ok executable'; echo 'ok service-spec'\n`);
  chmodSync(agent, 0o755);
  const line = checkLine.replaceAll("/src1", repo).replaceAll("/out", join(root, "out"));
  return spawnSync("bash", ["-c", line], { encoding: "utf8", env: { ...process.env, v_new: wanted } });
}

test("the container script checks N+1's agent version", () => assert.ok(checkLine, "no check line found"));

test("a multi-line self-test report with the expected version passes", () => {
  const r = run("0.1.1", "0.1.1");
  assert.equal(r.status, 0, r.stdout + r.stderr);
});

test("a different version fails with the whole report", () => {
  const r = run("0.1.0", "0.1.1");
  assert.equal(r.status, 1);
  assert.match(r.stdout, /reports version '0\.1\.0', expected '0\.1\.1'/);
});
