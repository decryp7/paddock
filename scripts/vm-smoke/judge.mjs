// The pure core of the VM start check (scripts/vm-smoke): parsers for what the guest prints (ps, launchctl, a discovery file) and
// the judges that turn those snapshots into pass/fail with evidence. No I/O, no clock: the orchestrator (run.mjs) collects the
// snapshots and the tests here feed it text. Process identity is pid plus start time (rule 53), never a bare pid.

/**
 * Parses `ps -axww -o pid=,ppid=,etime=,lstart=,command=` (macOS). etime is [[dd-]hh:]mm:ss; `lstart` is the process's start time as
 * ps prints it ("Fri Oct 30 14:25:12 2026", absent in older captures), which with the pid identifies a process (rule 53).
 * @returns {{pid:number, ppid:number, etime:string, ageSec:number, lstart:string|null, command:string}[]}
 */
export function parsePs(text) {
  const out = [];
  for (const line of String(text ?? '').split('\n')) {
    const m = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(?:([A-Z][a-z]{2} [A-Z][a-z]{2}\s+\d+ \d\d:\d\d:\d\d \d{4})\s+)?(.*\S)\s*$/.exec(line);
    if (!m) continue;
    out.push({ pid: Number(m[1]), ppid: Number(m[2]), etime: m[3], ageSec: etimeSeconds(m[3]), lstart: m[4] ?? null, command: m[5] });
  }
  return out;
}

/** `[[dd-]hh:]mm:ss` to seconds; NaN when it is not that shape. */
export function etimeSeconds(etime) {
  const m = /^(?:(\d+)-)?(?:(\d+):)?(\d+):(\d+)$/.exec(String(etime));
  if (!m) return Number.NaN;
  return Number(m[1] ?? 0) * 86400 + Number(m[2] ?? 0) * 3600 + Number(m[3]) * 60 + Number(m[4]);
}

/** The first word of a ps command line that is the executable (paths in the bundle have no spaces; a bundle's own path is checked as a prefix). */
function executableOf(command, bundleDir) {
  if (command.startsWith(`${bundleDir}/`)) {
    const rest = command.slice(bundleDir.length + 1);
    return `${bundleDir}/${rest.split(/\s/)[0]}`;
  }
  return command.split(/\s/)[0];
}

/**
 * The live processes of one declared process spec: the installed bundle's Contents/MacOS/<name>, with the optional args pattern.
 * @param {ReturnType<typeof parsePs>} procs
 * @param {{name:string, argsPattern?:string}} spec
 * @param {string} bundleDir e.g. /Applications/Dyno.app
 */
export function instancesOf(procs, spec, bundleDir) {
  const exe = `${bundleDir}/Contents/MacOS/${spec.name}`;
  const re = spec.argsPattern ? new RegExp(spec.argsPattern) : null;
  return procs.filter((p) => executableOf(p.command, bundleDir) === exe && (!re || re.test(p.command.slice(exe.length))));
}

/** Names of the config's processes that run from the bundle in this snapshot, with their pids: the compact evidence line. */
export function describeTree(procs, config, bundleDir) {
  const lines = [];
  for (const spec of config.processes) {
    for (const p of instancesOf(procs, spec, bundleDir)) {
      const parent = procs.find((q) => q.pid === p.ppid);
      lines.push(`${spec.name} pid ${p.pid} age ${p.etime} parent ${p.ppid}${p.ppid === 1 ? ' (launchd)' : parent ? ` (${parent.command.split(/\s/)[0].split('/').pop()})` : ' (gone)'}`);
    }
  }
  return lines.length ? lines : ['(none of the app\'s processes)'];
}

/**
 * Judges one snapshot against the app's declared processes.
 * - every spec has between `min` (default 1 for a snapshot that must be running) and `max` instances;
 * - each instance's parent is an allowed one: the app process ("app"), or launchd (ppid 1) only where the spec lists "launchd"
 *   and `loginService` is on.
 * @param {{running: boolean, loginService?: boolean}} want `running` false: nothing of the app may remain.
 * @returns {{ok:boolean, checks:{id:string, ok:boolean, detail:string}[]}}
 */
export function judgeSnapshot(procs, config, bundleDir, want) {
  const checks = [];
  const appSpec = config.processes.find((s) => s.role === 'app');
  const appPids = new Set(instancesOf(procs, appSpec, bundleDir).map((p) => p.pid));
  for (const spec of config.processes) {
    const found = instancesOf(procs, spec, bundleDir);
    const min = want.running ? (spec.min ?? 1) : 0;
    const max = want.running ? (spec.max ?? 1) : 0;
    const countOk = found.length >= min && found.length <= max;
    checks.push({
      id: `count:${spec.name}`,
      ok: countOk,
      detail: `${spec.name}: ${found.length} running (allowed ${min}..${max}); ${found.map((p) => `pid ${p.pid}`).join(', ') || 'none'}`,
    });
    if (!want.running || spec.role === 'app') continue;
    for (const p of found) {
      const viaApp = (spec.parents ?? ['app']).includes('app') && appPids.has(p.ppid);
      const viaLaunchd = (spec.parents ?? []).includes('launchd') && want.loginService === true && p.ppid === 1;
      // The app's own launchd job (Dyno ADR 0026: `net.decryptology.dyno.agent.ondemand`, bootstrapped even with Start at login off):
      // launchd is a legal parent only when launchd itself reports that job with this very pid (identity, never "ppid is 1").
      const jobPid = spec.parentJob ? procs.jobs?.[spec.parentJob]?.pid ?? null : null;
      const viaJob = Boolean(spec.parentJob) && p.ppid === 1 && jobPid === p.pid;
      checks.push({
        id: `parent:${spec.name}:${p.pid}`,
        ok: viaApp || viaLaunchd || viaJob,
        detail: `${spec.name} pid ${p.pid} has parent pid ${p.ppid}: ${viaApp ? 'the app' : viaLaunchd ? 'launchd (Launch at login is on)' : viaJob ? `launchd, as the job ${spec.parentJob} (launchd reports pid ${jobPid})` : 'not allowed (expected the app' + (want.loginService ? ' or launchd' : '') + (spec.parentJob && p.ppid === 1 ? `; launchd does not report job ${spec.parentJob} with pid ${p.pid}` : '') + ')'}`,
      });
    }
  }
  return { ok: checks.every((c) => c.ok), checks };
}

/** The highest instance count per spec over a series of snapshots (the observation window): a respawn that comes 10 s late shows here. */
export function peakCounts(snapshots, config, bundleDir) {
  const peak = Object.fromEntries(config.processes.map((s) => [s.name, 0]));
  for (const procs of snapshots) {
    for (const spec of config.processes) peak[spec.name] = Math.max(peak[spec.name], instancesOf(procs, spec, bundleDir).length);
  }
  return peak;
}

/**
 * How many distinct processes of each spec existed over a whole step: identity is the pid plus the start time ps printed
 * (`lstart`, taken from the guest, never host time minus an age), so a duplicate that exits before the app is ready, or an agent that
 * dies and is replaced, is counted although no single snapshot shows two. Without `lstart` (an old capture) the pid alone counts.
 * @param {{at:number, procs:ReturnType<typeof parsePs>}[]} trace snapshots
 */
export function distinctCounts(trace, config, bundleDir) {
  const out = {};
  for (const spec of config.processes) {
    const seen = new Set();
    for (const { procs } of trace) for (const p of instancesOf(procs, spec, bundleDir)) seen.add(`${p.pid}|${p.lstart ?? ''}`);
    out[spec.name] = seen.size;
  }
  return out;
}

/** The checks for `distinctCounts`: more distinct processes than `max` over the step fails. */
export function judgeDistinct(trace, config, bundleDir) {
  const d = distinctCounts(trace, config, bundleDir);
  return config.processes.map((s) => ({ id: `distinct:${s.name}`, ok: d[s.name] <= (s.max ?? 1), detail: `${s.name}: ${d[s.name]} distinct process(es) (pid and start time) seen from launch to the end of the window (allowed ${s.max ?? 1})` }));
}

/** Judges an observation window: no snapshot exceeded any spec's `max`, and every judged snapshot had a legal parent. */
export function judgeWindow(snapshots, config, bundleDir, want) {
  const peak = peakCounts(snapshots, config, bundleDir);
  const checks = config.processes.map((s) => ({
    id: `peak:${s.name}`,
    ok: peak[s.name] <= (s.max ?? 1) && (peak[s.name] >= (s.min ?? 1) || !want.running),
    detail: `${s.name}: peak ${peak[s.name]} over ${snapshots.length} snapshots (allowed ${want.running ? s.min ?? 1 : 0}..${s.max ?? 1})`,
  }));
  for (const [i, procs] of snapshots.entries()) {
    for (const c of judgeSnapshot(procs, config, bundleDir, want).checks) if (c.id.startsWith('parent:') && !c.ok) checks.push({ ...c, id: `${c.id}@${i}` });
  }
  return { ok: checks.every((c) => c.ok), checks, peak };
}

/**
 * Orphans after Quit: any process of the app that is still alive, with its parent. After a Quit nothing may remain, whoever the
 * parent is (an orphan has ppid 1).
 */
export function orphansAfterQuit(procs, config, bundleDir) {
  return config.processes.flatMap((spec) => instancesOf(procs, spec, bundleDir).map((p) => ({ name: spec.name, pid: p.pid, ppid: p.ppid, etime: p.etime, orphan: p.ppid === 1 })));
}

/** `launchctl print gui/<uid>/<label>` to { state, pid, program } (any field null when the job is not loaded or the text lacks it). */
export function parseLaunchctlPrint(text) {
  const t = String(text ?? '');
  const field = (re) => re.exec(t)?.[1] ?? null;
  const pid = field(/^\s*pid = (\d+)\s*$/m);
  return { state: field(/^\s*state = (\S+)\s*$/m), pid: pid === null ? null : Number(pid), program: field(/^\s*program = (.+?)\s*$/m) };
}

/**
 * The discovery file (agent.json and the like) against the live helper instances: the pid it names must be one of them, and a
 * file that names a dead pid is stale. `content` null: the file is absent.
 * @returns {{present:boolean, pid:number|null, live:boolean, stale:boolean}}
 */
export function judgeStateFile(content, procs, spec, bundleDir) {
  if (content == null || content === '') return { present: false, pid: null, live: false, stale: false };
  let pid = null;
  try {
    const v = JSON.parse(content)[spec.jsonPidField ?? 'pid'];
    pid = Number.isInteger(v) ? v : null;
  } catch {
    pid = null;
  }
  const helper = { name: spec.helper };
  const live = pid !== null && instancesOf(procs, helper, bundleDir).some((p) => p.pid === pid);
  return { present: true, pid, live, stale: !live };
}

/**
 * Classifies one failing step as `product` or `check`, with the evidence for the choice (ADR 0054 §1.5):
 * - `check`: no positive control was declared, the control failed, the probe could not run, or the reader found no element of the
 *   expected kind;
 * - `product`: the control passed, the probe ran and read a state that differs from the declaration.
 * @param {{control?: {ok:boolean, detail:string, raw?:string}|null, probe?: {ran:boolean, error?:string, foundElement?:boolean, raw?:string}, mismatch:string}} f
 * @returns {{class:'product'|'check', reason:string, evidence:string[], fix?:string}}
 */
export function classifyFailure({ control, probe = { ran: true }, mismatch }) {
  const raw = [`probe: ${probe.raw ?? '(no raw output)'}`, `control: ${control ? `${control.ok ? 'passed' : 'FAILED'}: ${control.detail}${control.raw ? ` (${control.raw})` : ''}` : 'none declared'}`];
  if (!control) return { class: 'check', reason: 'no positive control is declared for this check, so a failure cannot be told from a bug in the check', evidence: raw, fix: 'declare a positive control for this check (lifecycle.mjs defineCheck)' };
  if (!control.ok) return { class: 'check', reason: `the positive control failed (${control.detail}): the probe or judge cannot read a state known to pass`, evidence: raw, fix: 'fix the probe or the judge of this check; the product was not judged' };
  if (!probe.ran) return { class: 'check', reason: `the probe could not run${probe.error ? `: ${probe.error}` : ''}`, evidence: raw, fix: 'fix how the probe is started or reached in the guest' };
  if (probe.foundElement === false) return { class: 'check', reason: 'the reader found no element of the expected kind (wrong window, label or page)', evidence: raw, fix: 'fix the reader\'s selector or the page handle it holds' };
  return { class: 'product', reason: mismatch, evidence: raw };
}

/**
 * A step judged with a positive control. `status` is `pass`, `fail` (class product) or `broken` (class check); a broken step never
 * counts as a product FAIL and never lets the gate pass.
 * @param {string} title
 * @param {{control: {ok:boolean, detail:string, raw?:string}|null, probe?: object, checks: {ok:boolean, detail:string}[], pass?: string}} s
 */
export function controlledStep(title, { control, probe = { ran: true }, checks, pass = '' }) {
  const bad = checks.filter((c) => !c.ok);
  if (!bad.length && control?.ok && probe.ran !== false) return { title, status: 'pass', summary: pass, checks, control, class: null, evidence: [] };
  const c = classifyFailure({ control, probe, mismatch: bad.map((b) => b.detail).join('; ') || 'the probe read a state that differs from the declaration' });
  return { title, status: c.class === 'check' ? 'broken' : 'fail', class: c.class, summary: `${c.class === 'check' ? 'CHECK BROKEN: ' : ''}${c.reason}`.slice(0, 240), checks, control, evidence: c.evidence, fix: c.fix };
}

/**
 * Applies a reader control to steps that failed without one (the older start checks): a `fail` whose control passed stays a product
 * FAIL with the control as evidence; a `fail` whose control failed becomes `broken`.
 */
export function withReaderControl(step, control) {
  if (step.status !== 'fail' || step.class) return step;
  const c = classifyFailure({ control, probe: { ran: true, raw: (step.checks ?? []).filter((x) => !x.ok).map((x) => x.detail).join('; ') }, mismatch: step.summary });
  return { ...step, status: c.class === 'check' ? 'broken' : 'fail', class: c.class, summary: c.class === 'check' ? `CHECK BROKEN: ${c.reason}` : step.summary, control, evidence: [...(step.evidence ?? []), ...c.evidence], fix: c.fix };
}

/**
 * The verdict of a run. `product` FAILs and `check` errors (broken steps) both block a tag, and are reported apart.
 * @returns {{result:'PASS'|'FAIL'|'CHECK BROKEN'|'FAIL and CHECK BROKEN', product:string[], check:string[], blocksTag:boolean}}
 */
export function verdict(steps) {
  const product = steps.filter((s) => s.status === 'fail').map((s) => s.title);
  const check = steps.filter((s) => s.status === 'broken').map((s) => s.title);
  const result = product.length && check.length ? 'FAIL and CHECK BROKEN' : product.length ? 'FAIL' : check.length ? 'CHECK BROKEN' : 'PASS';
  return { result, product, check, blocksTag: product.length > 0 || check.length > 0 };
}

/** The step list of a run to Markdown: the broker's stamp, the verdict, a table of pass/fail per step, then each step's class and evidence. */
export function renderResult(run) {
  const mark = (s) => ({ pass: 'PASS', fail: 'FAIL', broken: 'CHECK BROKEN', skip: 'SKIP', manual: 'MANUAL', info: 'INFO' })[s] ?? s.toUpperCase();
  const v = verdict(run.steps);
  const lines = [
    `# VM start check: ${run.app} ${run.version ?? ''}`.trimEnd(),
    '',
    `- Result: **${v.result}**${run.steps.some((s) => s.status === 'manual') ? ' (manual steps are listed below and are not part of this verdict)' : ''}`,
    ...(v.product.length ? [`- Product FAILs (${v.product.length}): ${v.product.join('; ')}`] : []),
    ...(v.check.length ? [`- Check errors (${v.check.length}), not product failures; they block a tag until the check is fixed: ${v.check.join('; ')}`] : []),
    `- Installer: ${run.installer}`,
    `- Guest: ${run.guest}`,
    ...(run.lease ? [`- Broker lease: ${run.lease}`] : []),
    ...(run.vm ? [`- VM: ${run.vm}${run.snapshot ? `, snapshot ${run.snapshot}` : ''}`] : []),
    ...(run.command ? [`- Runner command: \`${run.command}\``] : []),
    `- Started: ${run.started}`,
    `- Tool: scripts/vm-smoke (commit ${run.commit ?? 'unknown'})`,
    '',
    '| # | Step | Result | Summary |',
    '|---|---|---|---|',
  ];
  run.steps.forEach((s, i) => lines.push(`| ${i + 1} | ${s.title} | ${mark(s.status)} | ${s.summary.replace(/\|/g, '\\|')} |`));
  lines.push('');
  run.steps.forEach((s, i) => {
    lines.push(`## ${i + 1}. ${s.title}: ${mark(s.status)}`, '');
    if (s.class) lines.push(`- Class: **${s.class}**${s.fix ? `; check to fix: ${s.fix}` : ''}`);
    if (s.control) lines.push(`- Positive control: ${s.control.ok ? 'passed' : 'FAILED'}: ${s.control.detail}`);
    for (const c of s.checks ?? []) lines.push(`- ${c.ok ? 'ok' : 'FAIL'}: ${c.detail}`);
    if (s.evidence?.length) lines.push('', '```', ...s.evidence, '```');
    lines.push('');
  });
  return `${lines.join('\n')}\n`;
}
