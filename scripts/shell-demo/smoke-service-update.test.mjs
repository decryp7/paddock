// The version pre-check of smoke-service-update.sh (macOS only: PlistBuddy). Fake .app bundles whose agent prints the same
// multi-line report as paddock_proc::self_test; a fake launchctl and a temp HOME keep the machine untouched (ER-14).
import test from "node:test";
import assert from "node:assert/strict";
import { chmodSync, mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const script = new URL("./smoke-service-update.sh", import.meta.url).pathname;
const mac = process.platform === "darwin";

function app(root, name, version, agentVersion) {
  const dir = join(root, name, "Paddock Shell Demo.app", "Contents");
  mkdirSync(join(dir, "MacOS"), { recursive: true });
  writeFileSync(
    join(dir, "Info.plist"),
    `<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>CFBundleShortVersionString</key><string>${version}</string></dict></plist>`,
  );
  const agent = join(dir, "MacOS", "shell-demo-agent");
  writeFileSync(
    agent,
    `#!/bin/sh\ncase "$1" in --self-test) echo 'shell-demo-agent ${agentVersion} self-test'; echo 'ok executable'; echo 'ok service-spec';; install) echo REACHED-INSTALL; exit 1;; esac\n`,
  );
  chmodSync(agent, 0o755);
  return join(root, name, "Paddock Shell Demo.app");
}

function run(agentOld, agentNew) {
  const root = mkdtempSync(join(tmpdir(), "smoke-update-"));
  const bin = join(root, "bin");
  mkdirSync(bin);
  writeFileSync(join(bin, "launchctl"), "#!/bin/sh\nexit 0\n");
  chmodSync(join(bin, "launchctl"), 0o755);
  const old = app(root, "old", "0.1.0", agentOld);
  const nw = app(root, "new", "0.1.1", agentNew);
  return spawnSync("bash", [script, old, nw], {
    encoding: "utf8",
    env: { ...process.env, PADDOCK_DISPOSABLE_MAC: "1", HOME: root, PADDOCK_SMOKE_APPS: join(root, "apps"), PATH: `${bin}:${process.env.PATH}` },
  });
}

test("agents reporting the app versions in a multi-line self-test pass the pre-check", { skip: !mac }, () => {
  const r = run("0.1.0", "0.1.1");
  assert.match(r.stdout, /REACHED-INSTALL/, r.stdout + r.stderr);
});

test("a version mismatch is one error line naming both versions, then the report", { skip: !mac }, () => {
  const r = run("0.1.0", "0.1.0");
  assert.equal(r.status, 1);
  const err = r.stdout.split("\n").filter((l) => l.startsWith("::error::"));
  assert.equal(err.length, 1);
  assert.match(err[0], /agent reports version '0\.1\.0' but the app is 0\.1\.1; rebuild/);
  assert.match(r.stdout, /ok service-spec/);
});
