// Lifecycle checks L1 to L8 with a positive control each (ADR 0054 §1.5). A check that cannot show it can pass is a check that cannot be
// believed when it fails: on 2026-11-04 a label matched too strictly, the Settings window was read as the page, the page handle was lost
// across a reboot, and a positive control itself failed, and all of it was reported as product failures.
//
// A check is { id, title, control, probe(io), judge(result) }:
// - `probe(io)` reads the guest and returns { ran, raw, value, error?, foundElement? };
// - `judge(value)` (the probe's `value`) returns { ok, checks: [{ ok, detail }] } for the state the probe read;
// - `control` is { describe, arrange(io), cleanup?(io), probe?, judge? }: it puts the guest (or a fake) in a state known to pass and
//   runs the same probe and judge on it. A check without a control is refused at definition time.
// `runCheck` runs the control first and the real probe after it, and hands both to `controlledStep` (judge.mjs), which classifies a
// failure as `product` or `check` and never lets a broken check count as a product FAIL.
import { controlledStep } from './judge.mjs';

export const LIFECYCLE_IDS = ['L1', 'L2', 'L3', 'L4', 'L5', 'L6', 'L7', 'L8'];

/** Validates and freezes a check definition. Throws when the control is missing or incomplete. */
export function defineCheck(def) {
  const where = `check ${def?.id ?? '(no id)'}`;
  if (!def || typeof def.id !== 'string' || typeof def.title !== 'string') throw new Error(`${where}: id and title are needed`);
  if (typeof def.probe !== 'function' || typeof def.judge !== 'function') throw new Error(`${where}: probe and judge are needed`);
  const c = def.control;
  if (!c || typeof c.describe !== 'string' || typeof c.arrange !== 'function') throw new Error(`${where}: a positive control ({ describe, arrange }) is needed; a check that cannot show it can pass cannot be believed`);
  return Object.freeze({ ...def });
}

/** The lifecycle suite's checks: every L1..L8 id once, each defined with a control. */
export function defineSuite(defs) {
  const ids = defs.map((d) => d.id);
  const dup = ids.find((id, i) => ids.indexOf(id) !== i);
  if (dup) throw new Error(`check ${dup} is defined twice`);
  return defs.map(defineCheck);
}

async function probeOnce(probe, io) {
  try {
    const r = await probe(io);
    return { ran: r?.ran !== false, raw: String(r?.raw ?? ''), error: r?.error, foundElement: r?.foundElement, value: r?.value };
  } catch (e) {
    return { ran: false, raw: '', error: e?.message ?? String(e) };
  }
}

/** Runs the control: arrange a known-pass state, probe, judge, clean up. Never throws. */
export async function runControl(def, io) {
  const c = def.control;
  try {
    await c.arrange(io);
  } catch (e) {
    return { ok: false, detail: `${c.describe}: could not arrange the known-pass state: ${e?.message ?? e}` };
  }
  try {
    const p = await probeOnce(c.probe ?? def.probe, io);
    if (!p.ran) return { ok: false, detail: `${c.describe}: the probe did not run${p.error ? `: ${p.error}` : ''}`, raw: p.raw };
    if (p.foundElement === false) return { ok: false, detail: `${c.describe}: the reader found no element of the expected kind`, raw: p.raw };
    const j = (c.judge ?? def.judge)(p.value);
    return { ok: j.ok, detail: `${c.describe}: ${j.ok ? 'the probe read the known-pass state as passing' : j.checks.filter((x) => !x.ok).map((x) => x.detail).join('; ')}`, raw: p.raw.slice(0, 300) };
  } finally {
    try {
      await c.cleanup?.(io);
    } catch {
      // a failed clean-up of a control must not hide the step; the guest is reverted at the end of the run
    }
  }
}

/** Runs one check: control first, then the real probe. Returns a step (pass, fail with class product, or broken with class check). */
export async function runCheck(def, io) {
  const control = await runControl(def, io);
  const p = await probeOnce(def.probe, io);
  const checks = p.ran && p.foundElement !== false ? def.judge(p.value).checks : [];
  return controlledStep(`${def.id}: ${def.title}`, { control, probe: { ran: p.ran, error: p.error, foundElement: p.foundElement, raw: p.raw.slice(0, 600) }, checks, pass: def.pass ?? '' });
}

/** Control for a registry value probe: the runner writes a value itself and the probe must read it back. */
export function runValueControl({ name = 'PaddockControl', value = 'control-value' } = {}) {
  return {
    describe: `Run-value probe on a value the runner wrote itself (${name})`,
    arrange: (io) => io.writeRunValue(name, value),
    cleanup: (io) => io.removeRunValue(name),
    probe: async (io) => ({ ran: true, raw: String(await io.readRunValue(name)), value: await io.readRunValue(name) }),
    judge: (v) => ({ ok: v === value, checks: [{ ok: v === value, detail: `read ${JSON.stringify(v)}, wrote ${JSON.stringify(value)}` }] }),
  };
}

/** Control for a page reader: a page whose label is known must be found by the same reader. */
export function pageReaderControl({ label }) {
  return {
    describe: `page reader on a known label (${JSON.stringify(label)})`,
    arrange: (io) => io.showKnownPage(label),
    probe: async (io) => {
      const text = await io.readPage();
      return { ran: true, raw: text.slice(0, 200), foundElement: text.length > 0, value: text };
    },
    judge: (text) => ({ ok: text.includes(label), checks: [{ ok: text.includes(label), detail: `the page text ${text.includes(label) ? 'has' : 'lacks'} ${JSON.stringify(label)}` }] }),
  };
}

/** Control for a page handle after a reboot: with the app known to be up, the handle must be re-acquired and answer. */
export function pageHandleControl() {
  return {
    describe: 'page handle re-acquired after a reboot of a guest whose app is up',
    arrange: (io) => io.ensureAppUp(),
    probe: async (io) => {
      const handle = await io.acquirePage();
      const answer = handle ? await handle.evaluate('1 + 1') : null;
      return { ran: true, raw: `handle ${handle ? 'acquired' : 'missing'}, answer ${answer}`, foundElement: Boolean(handle), value: answer };
    },
    judge: (answer) => ({ ok: answer === 2, checks: [{ ok: answer === 2, detail: `the handle answered ${answer}` }] }),
  };
}
