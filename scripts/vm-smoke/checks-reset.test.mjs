import test from 'node:test'; import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { Check } from './checks.mjs';
const config = JSON.parse(readFileSync(join(dirname(fileURLToPath(import.meta.url)), '..', '..', 'apps', 'dyno', 'vm-smoke.json'), 'utf8'));
// A guest that behaves as macOS 26 does: `ps -o lstart=` pads with two spaces; launchd supervises the agent and restarts it.
function guest({ trimsInGuest = false } = {}) {
  let alive = true, file = true, t = 0; const calls = [];
  const g = { now: () => (t += 1000), sleep: async () => {}, calls, state: () => ({ alive, file }),
    exec: async (c) => { calls.push(c); const ok = (stdout = '') => ({ code: 0, stdout, stderr: '' });
      if (c.startsWith('printf %s "$HOME"')) return ok('/Users/admin');
      if (c.startsWith('ps -axww')) return ok(alive ? '  853     1    01:08 Thu Nov  5 10:57:43 2026     dyno-agent run\n' : '');
      if (c.startsWith('launchctl print')) return ok(alive ? '\tstate = running\n\tpid = 853\n' : '');
      const k = /^\[ "\$\(ps -o lstart= -p \d+( \| sed -e 's\/ \*\$\/\/')?\)" = '(.*)' \]/.exec(c);
      // the guest's real comparison: `ps -o lstart= -p` prints the padded text, unless the command trims it
      if (k && !(k[1] && !trimsInGuest)) { if (k[2] === (k[1] ? 'Thu Nov  5 10:57:43 2026' : 'Thu Nov  5 10:57:43 2026  ')) alive = false; return ok(); }
      if (k) return ok();
      if (c.startsWith('rm -f') && c.includes('agent.json')) { file = false; return ok(); }
      return ok(); } };
  return g;
}
test('reset before a relaunch leaves a supervised agent and its discovery file together', async () => {
  const g = guest(); const c = new Check(g, config); c.loginOn = true;
  await c.reset();
  assert.deepEqual(g.state(), { alive: true, file: true }, 'the launchd agent runs on and can still be found');
});

test('a kill that matches ends the agent and then its discovery file goes too; a failed kill keeps both', async () => {
  const g = guest({ trimsInGuest: true }); const c = new Check(g, config); c.loginOn = true;
  const r = await c.reset();
  assert.deepEqual(g.state(), { alive: false, file: false }, 'the start time is compared trimmed, so the identity check passes and the agent ends');
  assert.deepEqual(r.kept, []);
  const h = guest(); const d = new Check(h, config); d.loginOn = true;
  const r2 = await d.reset();
  assert.equal(r2.gone, false);
  assert.ok(r2.kept.length > 0, 'the survivor is named so the step reports it');
});
