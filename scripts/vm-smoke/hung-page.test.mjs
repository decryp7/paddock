import assert from 'node:assert/strict';
import { test } from 'node:test';
import { eventOf, hungPageStep, judgeHung } from './hung-page.mjs';

const bad = (c) => c.filter((x) => !x.ok);
const good = { suspendedAt: 0, resumedAt: 300, events: [{ t: 11, kind: 'marked' }, { t: 62, kind: 'asked' }, { t: 190, kind: 'asked' }, { t: 304, kind: 'recovered' }], questionsWhileOpen: 1, questionsAfterResume: 0, keepWaitingAt: 65, secondAskAt: 190 };
test('the design timing passes', () => assert.equal(bad(judgeHung(good)).length, 0));
test('dead state: no mark, no question, an early question, two questions, none on screen fail', () => {
  assert.ok(bad(judgeHung({ ...good, events: [] })).length);
  assert.ok(bad(judgeHung({ ...good, events: [{ t: 11, kind: 'marked' }, { t: 20, kind: 'asked' }] })).some((c) => /too early/.test(c.detail)));
  assert.ok(bad(judgeHung({ ...good, events: [{ t: 11, kind: 'marked' }, { t: 62, kind: 'asked' }, { t: 70, kind: 'asked' }], keepWaitingAt: null })).some((c) => /2 questions/.test(c.detail)));
  assert.ok(bad(judgeHung({ ...good, questionsWhileOpen: 2 })).length);
  assert.ok(bad(judgeHung({ ...good, questionsWhileOpen: 0 })).length);
  assert.equal(judgeHung({ ...good, questionsWhileOpen: null }).find((c) => c.kind === 'check').ok, false);
});
test('dead state: Keep Waiting that does not double, no recovery, or a question left after the resume fail', () => {
  assert.ok(bad(judgeHung({ ...good, secondAskAt: 100 })).some((c) => /did not double/.test(c.detail)));
  assert.ok(bad(judgeHung({ ...good, secondAskAt: null })).length);
  assert.ok(bad(judgeHung({ ...good, events: good.events.slice(0, 3) })).length);
  assert.ok(bad(judgeHung({ ...good, questionsAfterResume: 1 })).length);
  assert.equal(judgeHung({ ...good, keepWaitingAt: null, secondAskAt: null }).find((c) => /could not be pressed/.test(c.detail)).kind, 'check');
});
test('log lines map to events', () => {
  assert.equal(eventOf('Window main stopped responding; the user is asked if it stays silent for a long time'), 'marked');
  assert.equal(eventOf('Window main has not answered for a long time; asking the user'), 'asked');
  assert.equal(eventOf('Not-responding question answered'), 'answered');
  assert.equal(eventOf('Window main responds again'), 'recovered');
  assert.equal(eventOf('something else'), null);
});
test('the step runs against a fake guest on a fake clock; a process that cannot be stopped is a broken check', async () => {
  let t = 0; const log = []; let asked = 0, kw = false, resumed = false;
  const g = { now: () => t, sleep: async (ms) => { t += ms / 1000; if (t >= 11 && !log.some((l) => /stopped responding/.test(l))) log.push('Window main stopped responding; the user is asked if it stays silent for a long time'); const due = kw ? 62 + 128 : 62; if (!resumed && t >= due + (kw ? 3 : 0) && asked < (kw ? 2 : 1)) { asked++; log.push(`Window main has not answered for a long time; asking the user #${asked}`); } if (resumed && !log.some((l) => /responds again/.test(l))) log.push('Window main responds again'); },
    suspend: async () => {}, resume: async () => { resumed = true; }, logLines: async () => [...log], questions: async () => (resumed ? 0 : 1), keepWaiting: async () => { kw = true; return true; } };
  const r = await hungPageStep(g, 'x');
  assert.equal(r.status, 'pass', r.summary);
  assert.equal((await hungPageStep({ ...g, suspend: async () => { throw new Error('no such process'); } }, 'x')).status, 'check-broken');
});
test('lines the polls never returned are taken from the log by their own time', async () => {
  let t = 1000; let resumed = false;
  const at = (s) => new Date((1000 + s) * 1000).toISOString();
  const lines = [`{"@m":"Window main stopped responding; the user is asked if it stays silent for a long time","@t":"${at(12)}"}`, `{"@m":"Window main has not answered for a long time; asking the user","@t":"${at(62)}"}`, `{"@m":"Window main responds again","@t":"${at(400)}"}`];
  const g = { clockOffsetMs: 0, now: () => t, sleep: async (ms) => { t += ms / 1000; }, suspend: async () => {}, resume: async () => { resumed = true; },
    logLines: async () => [lines[0]], finalLines: async () => lines, questions: async () => (resumed ? 0 : 1), keepWaiting: async () => false };
  const r = await hungPageStep(g, 'x');
  const bad = r.checks.filter((c) => !c.ok && c.kind !== 'check');
  assert.deepEqual(bad.map((c) => c.detail), [], 'no product verdict from a poll that missed lines');
  assert.ok(r.checks.some((c) => /first question came 6\d/.test(c.detail)));
});
