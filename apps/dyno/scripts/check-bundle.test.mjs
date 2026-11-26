import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { chmodSync, mkdtempSync, readFileSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { test, after } from 'node:test';

// The agent-launch step of check-bundle.sh (`--launch-view <agent>`), against a stand-in `launchctl` (nothing real is started, ER-14):
// nightly 9103's Dyno bundle job failed there because the CI Mac is a LaunchDaemon with no login session (ADR 0120, ER-60).
const script = fileURLToPath(new URL('./check-bundle.sh', import.meta.url));
const dir = mkdtempSync(join(tmpdir(), 'dyno-check-bundle-'));
after(() => rmSync(dir, { recursive: true, force: true }));

const stub = (name, body) => {
  writeFileSync(join(dir, name), `#!/usr/bin/env bash\n${body}\n`);
  chmodSync(join(dir, name), 0o755);
};
// `print gui/<uid>` is the probe for a login session; `submit` records its call and writes the view's line into the -o file.
stub(
  'launchctl',
  `case "$1" in
  print) [ "\${STUB_SESSION:-1}" = 1 ] ;;
  submit) echo submit >> "$STUB_LOG"
    [ -z "\${STUB_SUBMIT_ERROR:-}" ] || { echo "$STUB_SUBMIT_ERROR" >&2; exit 1; }
    while [ $# -gt 0 ]; do [ "$1" = -o ] && out="$2"; shift; done
    echo "\${STUB_VIEW:-identifier=net.decryptology.dyno.agent announced=3 sent=3 cut_down=false}" > "$out" ;;
  *) exit 0 ;;
esac`,
);
stub('sw_vers', 'echo 15.1');

function run(env) {
  const log = join(dir, 'launchctl.log');
  rmSync(log, { force: true });
  const r = spawnSync('bash', [script, '--launch-view', '/not/a/real/agent'], {
    encoding: 'utf8',
    env: { ...process.env, PATH: `${dir}:${process.env.PATH}`, TMPDIR: dir, STUB_LOG: log, ...env },
  });
  return { code: r.status, out: `${r.stdout}${r.stderr}`, submitted: existsSync(log) ? readFileSync(log, 'utf8').trim().split('\n').length : 0 };
}

test('without a login session the step says NOT MEASURED, starts nothing and is not a pass or a failure', () => {
  const r = run({ STUB_SESSION: '0' });
  assert.equal(r.code, 0, r.out);
  assert.match(r.out, /::warning::.*NOT MEASURED.*no login session.*ER-60/);
  assert.equal(r.submitted, 0, 'launchctl submit must not be called');
  assert.doesNotMatch(r.out, /bundled agent socket view/, 'a skipped step never prints a measurement');
});

test('with a login session the agent view is measured and its line printed', () => {
  const r = run({});
  assert.equal(r.code, 0, r.out);
  assert.equal(r.submitted, 1);
  assert.match(r.out, /bundled agent socket view \(as a launchd job\): identifier=net\.decryptology\.dyno\.agent/);
  assert.doesNotMatch(r.out, /NOT MEASURED/);
});

test('a failing launchctl submit fails the check and prints what launchctl said', () => {
  const r = run({ STUB_SUBMIT_ERROR: 'Bad request: Could not find service' });
  assert.notEqual(r.code, 0);
  assert.match(r.out, /::error::launchctl could not start the bundled dyno-agent view: Bad request: Could not find service/);
});

test('a view that is not Dyno’s own identifier fails the check (the measured branch still judges)', () => {
  const r = run({ STUB_VIEW: 'identifier=com.example.other announced=1 sent=1 cut_down=false' });
  assert.notEqual(r.code, 0);
  assert.match(r.out, /code-signing identifier is not net\.decryptology\.dyno\./);
});
