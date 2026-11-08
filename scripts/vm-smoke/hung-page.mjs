// The hung or paused visible page row. Design (crates/paddock-startup, origin/dyno/wp-int f39189f09; watchdog.rs `Action::AskHung`, `Config::ask_timeout`, `max_doublings`):
// a visible page that was ready and stops answering is marked at about 10 s and asked about after about 60 s of silence (worst case about 62 s; tick jitter may move
// it); one question per window, none while one is open; each Keep Waiting doubles the wait (60, 120, 240, 480 s); it clears when the page answers again.
// The guest stops the page's content process (SIGSTOP, or a suspend on Windows); evidence is the app log lines ("stopped responding", "has not answered for a long time;
// asking the user", "Not-responding question answered", "responds again") seen with the time they were first observed, and the number of questions on screen.
// Pure judge + a step over injected primitives: no VM needed to test it.
import { makeResult } from './verdict.mjs';

export const DESIGN = { markAroundS: 10, askAfterS: 60, askWorstS: 62, doubling: 2, tolerance: 8 };

const chk = (ok, detail, kind) => ({ ok, detail, ...(kind ? { kind } : {}) });

/** Which watchdog event an app log line is. */
export function eventOf(line) {
  if (/stopped responding; the user is asked/i.test(line)) return 'marked';
  if (/has not answered for a long time; asking the user/i.test(line)) return 'asked';
  if (/Not-responding question answered/i.test(line)) return 'answered';
  if (/responds again/i.test(line)) return 'recovered';
  return null;
}

/**
 * @param {{suspendedAt:number, resumedAt:number|null, events:{t:number, kind:string}[], questionsWhileOpen:number|null, questionsAfterResume:number|null,
 *          keepWaitingAt:number|null, secondAskAt:number|null}} f times in seconds on one clock
 */
export function judgeHung(f) {
  const since = (t) => Math.round((t - f.suspendedAt) * 10) / 10;
  const first = (k) => f.events.find((e) => e.kind === k && e.t >= f.suspendedAt);
  const marked = first('marked'), asked = first('asked');
  const checks = [];
  checks.push(chk(Boolean(marked), marked ? `the page was marked unresponsive ${since(marked.t)} s after it was stopped` : 'the page was never marked unresponsive (no "stopped responding" line)'));
  if (!asked) { checks.push(chk(false, 'no question was asked about the stopped page (no "asking the user" line)')); return checks; }
  const a = since(asked.t);
  checks.push(chk(a >= DESIGN.askAfterS - DESIGN.tolerance, a >= DESIGN.askAfterS - DESIGN.tolerance ? `the first question came ${a} s after the stop (design: about ${DESIGN.askAfterS} s)` : `the first question came after only ${a} s (design: about ${DESIGN.askAfterS} s): too early`));
  checks.push(chk(a <= DESIGN.askWorstS + 4 * DESIGN.tolerance, a <= DESIGN.askWorstS + 4 * DESIGN.tolerance ? 'the first question was not far later than the design allows' : `the first question came ${a} s after the stop, long after the design's worst case`));
  const asks = f.events.filter((e) => e.kind === 'asked' && e.t >= f.suspendedAt && (f.keepWaitingAt === null || e.t < f.keepWaitingAt));
  checks.push(chk(asks.length === 1, asks.length === 1 ? 'exactly one question was logged before Keep Waiting' : `${asks.length} questions were logged for one window before Keep Waiting`));
  if (f.questionsWhileOpen === null) checks.push(chk(false, 'the number of questions on screen could not be read', 'check'));
  else checks.push(chk(f.questionsWhileOpen === 1, f.questionsWhileOpen === 1 ? 'exactly one question is on screen' : `${f.questionsWhileOpen} questions are on screen (expected exactly one)`));
  if (f.keepWaitingAt === null) checks.push(chk(false, 'Keep Waiting could not be pressed, so the doubling was not tested', 'check'));
  else if (f.secondAskAt === null) checks.push(chk(false, 'no second question came after Keep Waiting within the observation window'));
  else {
    const w = Math.round((f.secondAskAt - f.keepWaitingAt) * 10) / 10;
    const min = DESIGN.askAfterS * DESIGN.doubling - DESIGN.tolerance;
    checks.push(chk(w >= min, w >= min ? `after Keep Waiting the next question came ${w} s later (design: doubled, about ${DESIGN.askAfterS * DESIGN.doubling} s)` : `after Keep Waiting the next question came after only ${w} s (design: about ${DESIGN.askAfterS * DESIGN.doubling} s): the wait did not double`));
  }
  if (f.resumedAt === null) checks.push(chk(false, 'the page process was not resumed', 'check'));
  else {
    const rec = f.events.find((e) => e.kind === 'recovered' && e.t >= f.resumedAt);
    checks.push(chk(Boolean(rec), rec ? 'the app logged that the window responds again' : 'the app never logged that the window responds again after the resume'));
    if (f.questionsAfterResume === null) checks.push(chk(false, 'the question count after the resume could not be read', 'check'));
    else checks.push(chk(f.questionsAfterResume === 0, f.questionsAfterResume === 0 ? 'the question cleared when the page resumed' : `${f.questionsAfterResume} question(s) stayed on screen after the page resumed`));
  }
  return checks;
}

/**
 * The step. `g`: now() seconds, sleep(ms), suspend(), resume(), logLines() -> string[] (the app's log so far), questions() -> number|null (questions on screen),
 * keepWaiting() -> boolean (presses the button), shot(name) -> path|null. Observes by polling every 3 s (times are first-seen times).
 */
export async function hungPageStep(g, title) {
  const events = [];
  const seen = new Set();
  let polls = 0, lastLines = [];
  const poll = async () => { polls++; lastLines = await g.logLines(); for (const l of lastLines) { const k = eventOf(l); if (k && !seen.has(l)) { seen.add(l); events.push({ t: g.now(), kind: k }); } } };
  await poll(); // lines from before the stop are not this stop's
  events.length = 0;
  const suspendedAt = g.now();
  let guestMark = null;
  const toHost = (iso) => (g.guestAtSuspend && guestMark ? suspendedAt + (Date.parse(iso) - Date.parse(guestMark)) / 1000 : g.clockOffsetMs !== undefined ? (Date.parse(iso) + g.clockOffsetMs) / 1000 : null);
  try { await g.suspend(); guestMark = g.guestAtSuspend ? (await g.guestAtSuspend()) : null; } catch (e) { return makeResult(title, [chk(false, `the page process could not be stopped: ${e.message}`, 'check')]); }
  let questionsWhileOpen = null, keepWaitingAt = null, secondAskAt = null, shot = null;
  for (let i = 0; i < 90 && !events.some((e) => e.kind === 'asked'); i++) { await g.sleep(3000); await poll(); }
  // A question the polls did not return but the log holds (0ad round 7): dated by the log's own @t, moved to the host clock by the offset measured at the stop.
  if (!events.some((e) => e.kind === 'asked') && g.finalLines) {
    for (const l of await g.finalLines()) {
      const m = /"@t":"([^"]+)"/.exec(l);
      if (eventOf(l) === 'asked' && m && toHost(m[1]) !== null) { const t = toHost(m[1]); if (t >= suspendedAt - 5) { events.push({ t, kind: 'asked' }); break; } }
    }
  }
  if (events.some((e) => e.kind === 'asked')) {
    await g.sleep(2000);
    questionsWhileOpen = await g.questions('open');
    shot = await g.shot?.('hung-question');
    if (await g.keepWaiting()) {
      keepWaitingAt = g.now();
      const before = events.filter((e) => e.kind === 'asked').length;
      for (let i = 0; i < 60 && events.filter((e) => e.kind === 'asked').length === before; i++) { await g.sleep(3000); await poll(); }
      if (events.filter((e) => e.kind === 'asked').length > before) secondAskAt = events.filter((e) => e.kind === 'asked').at(-1).t;
    }
  }
  let resumedAt = null, questionsAfterResume = null;
  try { await g.resume(); resumedAt = g.now(); for (let i = 0; i < 10 && !events.some((e) => e.kind === 'recovered' && e.t >= resumedAt); i++) { await g.sleep(3000); await poll(); } await g.sleep(2000); questionsAfterResume = await g.questions('resumed'); } catch { /* reported by the judge */ }
  // Lines the polls did not return but the log holds are added here, dated by their own @t on the host clock, so a poll that misses lines cannot turn into a verdict about the app.
  const all = g.finalLines ? await g.finalLines() : [];
  for (const l of all) {
    const m = /"@t":"([^"]+)"/.exec(l); const k = eventOf(l);
    if (!k || !m || toHost(m[1]) === null) continue;
    const t = toHost(m[1]);
    if (t >= suspendedAt - 5 && !events.some((e) => e.kind === k && Math.abs(e.t - t) < 15)) events.push({ t, kind: k });
  }
  events.sort((a, b) => a.t - b.t);
  const lateAsk = all.filter((l) => eventOf(l) === 'asked');
  const diag = [`polls: ${polls}; last poll returned ${lastLines.length} line(s)`, ...(lateAsk.length && !events.some((e) => e.kind === 'asked') ? [`the log DOES hold an asking line the polls did not return: ${lateAsk[0].slice(0, 160)}`] : [])];
  return makeResult(title, judgeHung({ suspendedAt, resumedAt, events, questionsWhileOpen, questionsAfterResume, keepWaitingAt, secondAskAt }), [...(shot ? [`screenshot: ${shot}`] : []), ...diag], 'one question about 60 s after the stop, Keep Waiting doubled the wait, it cleared on resume');
}
