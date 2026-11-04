// The start check itself, against a `guest` (the Tart VM in run.mjs, a fake in checks.test.mjs). Every wait is a poll on state with a
// guard for a truly stuck step; the verdicts come from judge.mjs. The guest is a throw-away clone: the only processes the check ends
// are the app's own leftovers, and only after re-checking their identity in the same guest command (rule 53).
import { describeTree, instancesOf, judgeDistinct, judgeSnapshot, judgeStateFile, judgeWindow, orphansAfterQuit, parseLaunchctlPrint, parsePs, withReaderControl } from './judge.mjs';

const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;

/**
 * @typedef {{ exec(cmd: string, opts?: {timeoutMs?: number}): Promise<{code:number, stdout:string, stderr:string}>, now(): number, sleep(ms: number): Promise<void> }} Guest
 * `exec` runs a bash command line as the guest's user (in the guest's Aqua session on the pinned image; GUI apps are started with `open`).
 */

export class Check {
  /** @param {Guest} guest */
  constructor(guest, config, { bundleDir = `/Applications/${config.bundle}`, log = () => {}, intervalMs = 2000 } = {}) {
    this.g = guest;
    this.config = config;
    this.bundle = bundleDir;
    this.log = log;
    this.interval = intervalMs;
    this.home = null;
    this.trace = [];
  }

  async home_() {
    this.home ??= (await this.g.exec('printf %s "$HOME"')).stdout.trim();
    return this.home;
  }

  async path(p) {
    return p.replace(/^~/, await this.home_());
  }

  async snapshot() {
    const r = await this.g.exec('ps -axww -o pid=,ppid=,etime=,lstart=,command=');
    const procs = parsePs(r.stdout);
    // A helper with a launchd job of its own (spec.parentJob) that runs under launchd: ask launchd for the job, so the judge can check the pid.
    procs.jobs = {};
    for (const spec of this.config.processes) {
      if (spec.parentJob && instancesOf(procs, spec, this.bundle).some((p) => p.ppid === 1)) {
        procs.jobs[spec.parentJob] = parseLaunchctlPrint((await this.g.exec(`launchctl print gui/$(id -u)/${spec.parentJob} 2>&1`)).stdout);
      }
    }
    this.trace.push({ at: this.g.now(), procs });
    return procs;
  }

  /** Positive control for the process reader (ADR 0054 §1.5): `ps` must list itself with a pid and a start time, or a failing step is the check's bug. */
  async readerControl() {
    const r = await this.g.exec('ps -axww -o pid=,ppid=,etime=,lstart=,command=');
    const rows = parsePs(r.stdout);
    const self = rows.find((p) => /(^|\/)ps( |$)/.test(p.command));
    const ok = r.code === 0 && Boolean(self?.lstart);
    return { ok, detail: ok ? `ps lists itself (pid ${self.pid}, started ${self.lstart})` : 'ps did not list itself with a start time', raw: `exit ${r.code}, ${rows.length} rows` };
  }

  async readFile(p) {
    const r = await this.g.exec(`cat ${sq(await this.path(p))} 2>/dev/null`);
    return r.code === 0 ? r.stdout : null;
  }

  /** Polls `fn` (async, returns a truthy value when done) until it holds or `guardMs` of guest-independent time passes. */
  async until(fn, guardMs) {
    const t0 = this.g.now();
    for (;;) {
      const v = await fn();
      if (v) return { ok: true, value: v, tookMs: this.g.now() - t0 };
      if (this.g.now() - t0 >= guardMs) return { ok: false, value: v, tookMs: this.g.now() - t0 };
      await this.g.sleep(this.interval);
    }
  }

  /**
   * Ends the app's own leftover processes. Each is ended only if its pid, its start time (`ps -o lstart=`) and its command line
   * still match in the same guest command (rule 53). Returns what could not be cleaned: a step that starts from a dirty guest is
   * not trustworthy, so the callers fail on it.
   */
  async reset() {
    const procs = await this.snapshot();
    const left = this.config.processes.flatMap((s) => instancesOf(procs, s, this.bundle));
    const stillThere = async () => { const now = await this.snapshot(); return left.filter((p) => now.some((q) => q.pid === p.pid && q.command === p.command)); };
    const end = async (p, signal) => {
      if (!p.lstart) return; // no recorded start time: the identity cannot be checked, so nothing is ended
      await this.g.exec(`[ "$(ps -o lstart= -p ${p.pid})" = ${sq(p.lstart)} ] && ps -p ${p.pid} -o command= | grep -qxF -- ${sq(p.command)} && kill ${signal} ${p.pid}`);
    };
    for (const p of left) await end(p, '-TERM');
    // An agent that ignores TERM (the orphan of the 7207 run did) is ended with KILL, after the same identity check.
    await this.until(async () => (await stillThere()).length === 0, 8_000);
    for (const p of await stillThere()) await end(p, '-KILL');
    const gone = await this.until(async () => (await stillThere()).length === 0, 12_000);
    for (const f of this.config.stateFiles ?? []) await this.g.exec(`rm -f ${sq(await this.path(f.path))}`);
    return { ended: left.length, gone: gone.ok };
  }

  async start({ hidden = false } = {}) {
    const r = await this.g.exec(`open -a ${sq(this.bundle)}${hidden ? ' --args --hidden' : ''}`);
    return r;
  }

  /** True when every process that must run does, the state files name live helpers, and the ready command (if any) succeeds. */
  async isReady() {
    const procs = await this.snapshot();
    const j = judgeSnapshot(procs, this.config, this.bundle, { running: true, loginService: this.loginOn ?? false });
    const counts = j.checks.filter((c) => c.id.startsWith('count:')).every((c) => c.ok);
    if (!counts) return false;
    for (const f of this.config.stateFiles ?? []) {
      const s = judgeStateFile(await this.readFile(f.path), procs, f, this.bundle);
      if (!s.present || !s.live) return false;
    }
    if (this.config.ready) {
      const r = await this.g.exec(`${sq(`${this.bundle}/${this.config.ready.command[0]}`)} ${this.config.ready.command.slice(1).map(sq).join(' ')}`, { timeoutMs: 30_000 });
      if (r.code !== 0) return false;
    }
    return true;
  }

  /** Observes for `settleSec`: snapshots every interval, so a respawn that arrives late is seen. */
  async observe(want) {
    const snaps = [];
    const t0 = this.g.now();
    do {
      snaps.push(await this.snapshot());
      if (this.g.now() - t0 < this.config.settleSec * 1000) await this.g.sleep(this.interval);
    } while (this.g.now() - t0 < this.config.settleSec * 1000);
    return { snaps, ...judgeWindow(snaps, this.config, this.bundle, want) };
  }

  /** Start, wait for ready (guarded), then observe: the common body of the cold start, relaunch and login-service steps. */
  async startAndJudge({ hidden = false, loginService = false, title, pre } = {}) {
    this.loginOn = loginService;
    const cleaned = await this.reset();
    if (pre) await pre();
    this.trace = [];
    const opened = await this.start({ hidden });
    if (opened.code !== 0) return { title, status: 'fail', summary: `open failed (exit ${opened.code})`, checks: [{ ok: false, detail: opened.stderr.trim() || 'open printed nothing' }] };
    const ready = await this.until(() => this.isReady(), this.config.readyTimeoutSec * 1000);
    const after = await this.observe({ running: true, loginService });
    const procs = after.snaps.at(-1);
    // Every snapshot since the launch (the readiness polls included) is judged, and the distinct processes are counted over all of it.
    const whole = judgeWindow(this.trace.map((t) => t.procs), this.config, this.bundle, { running: true, loginService });
    const checks = [{ ok: cleaned.gone, detail: `guest clean before the start (${cleaned.ended} leftover(s) of an earlier step ended${cleaned.gone ? '' : ', but some did not go away'})` },
      { ok: ready.ok, detail: ready.ok ? `ready after ${(ready.tookMs / 1000).toFixed(0)} s (all processes up, discovery file names the live helper, ready command answers)` : `not ready within the ${this.config.readyTimeoutSec} s guard` }, ...whole.checks.filter((x) => x.id.startsWith('peak:') || x.id.startsWith('parent:')), ...judgeDistinct(this.trace, this.config, this.bundle)];
    for (const f of this.config.stateFiles ?? []) {
      const s = judgeStateFile(await this.readFile(f.path), procs, f, this.bundle);
      checks.push({ ok: s.present && s.live, detail: `${f.path}: ${s.present ? `names pid ${s.pid}, ${s.live ? 'a live ' + f.helper : 'NOT a live ' + f.helper + ' (stale)'}` : 'absent'}` });
    }
    const ok = checks.every((c) => c.ok);
    return { title, status: ok ? 'pass' : 'fail', summary: ok ? `one of each, peak ${JSON.stringify(whole.peak)}, no replacement or early duplicate` : checks.filter((c) => !c.ok).map((c) => c.detail).join('; ').slice(0, 200), checks, evidence: describeTree(procs, this.config, this.bundle) };
  }

  async quit(title) {
    const name = this.config.quit.appleScriptName;
    const q = await this.g.exec(`osascript -e ${sq(`tell application "${name}" to quit`)}`, { timeoutMs: 30_000 });
    const gone = await this.until(async () => orphansAfterQuit(await this.snapshot(), this.config, this.bundle).length === 0, 40_000);
    const procs = await this.snapshot();
    const left = orphansAfterQuit(procs, this.config, this.bundle);
    const checks = [
      { ok: q.code === 0, detail: q.code === 0 ? 'Quit sent (osascript)' : `Quit command failed (exit ${q.code}): ${q.stderr.trim().slice(0, 200)}` },
      { ok: gone.ok, detail: gone.ok ? `every process of the app ended ${(gone.tookMs / 1000).toFixed(0)} s after Quit` : 'processes of the app still running after the 40 s guard' },
      ...left.map((o) => ({ ok: false, detail: `left behind: ${o.name} pid ${o.pid} (parent ${o.ppid}${o.orphan ? ', an orphan of launchd' : ''}, age ${o.etime})` })),
    ];
    for (const spec of this.config.processes.filter((s) => s.parentJob)) {
      const lc = await this.g.exec(`launchctl print gui/$(id -u)/${spec.parentJob} >/dev/null 2>&1; echo $?`);
      const loaded = lc.stdout.trim() === '0';
      checks.push({ ok: !loaded, detail: `launchd job ${spec.parentJob}: ${loaded ? 'STILL LOADED after Quit (ADR 0026: the job must be gone)' : 'gone after Quit'}` });
    }
    for (const f of (this.config.stateFiles ?? []).filter((x) => x.removedAfterQuit)) {
      const c = await this.readFile(f.path);
      checks.push({ ok: c == null, detail: `${f.path}: ${c == null ? 'removed' : 'still there after Quit'}` });
    }
    const evidence = describeTree(procs, this.config, this.bundle);
    const ok = checks.every((c) => c.ok);
    if (!ok) await this.reset();
    return { title, status: ok ? 'pass' : 'fail', summary: ok ? 'nothing the app started is left' : checks.filter((c) => !c.ok).map((c) => c.detail).join('; ').slice(0, 200), checks, evidence };
  }

  async install(stagedDmg) {
    const title = 'Install';
    const mnt = '/tmp/vmsmoke-dmg';
    const cmds = [
      `hdiutil detach ${sq(mnt)} >/dev/null 2>&1; mkdir -p ${sq(mnt)}`,
      `hdiutil attach -nobrowse -readonly -mountpoint ${sq(mnt)} ${sq(stagedDmg)}`,
      `sudo rm -rf ${sq(this.bundle)} && ditto ${sq(`${mnt}/${this.config.bundle}`)} ${sq(this.bundle)}`,
      `hdiutil detach ${sq(mnt)}`,
    ];
    const checks = [];
    // The pinned cirruslabs image ships with Gatekeeper disabled (`spctl --status`: assessments disabled); a check that installs an
    // ad-hoc build on a stock Mac must run with it on, so it is switched on before anything is installed.
    await this.g.exec('sudo spctl --global-enable');
    const gk = (await this.g.exec('spctl --status 2>&1')).stdout.trim();
    checks.push({ ok: /assessments enabled/.test(gk), detail: `Gatekeeper enabled in the guest before the install (spctl --global-enable; the image ships with it off): \`${gk || 'no answer'}\`` });
    for (const c of cmds) {
      const r = await this.g.exec(c, { timeoutMs: 300_000 });
      if (r.code !== 0 && !c.startsWith('hdiutil detach')) {
        checks.push({ ok: false, detail: `${c.split(' ')[0]} failed (exit ${r.code}): ${(r.stderr || r.stdout).trim().slice(0, 200)}` });
        return { title, status: 'fail', summary: checks[0].detail, checks };
      }
    }
    const ver = await this.g.exec(`defaults read ${sq(`${this.bundle}/Contents/Info`)} CFBundleShortVersionString`);
    const seal = await this.g.exec(`codesign --verify --deep --strict ${sq(this.bundle)} 2>&1`, { timeoutMs: 300_000 });
    this.version = ver.stdout.trim();
    checks.push({ ok: ver.code === 0, detail: `installed ${this.config.bundle} version ${this.version || '(unreadable)'}` });
    checks.push({ ok: seal.code === 0, detail: `codesign --verify --deep --strict: ${seal.code === 0 ? 'valid' : seal.stdout.trim().slice(0, 200)}` });
    const ok = checks.every((c) => c.ok);
    return { title, status: ok ? 'pass' : 'fail', summary: ok ? `${this.config.bundle} ${this.version} installed, seal valid` : 'install or seal failed', checks };
  }

  async secondLaunch() {
    const title = 'Second launch while running keeps one instance';
    this.trace = [];
    const before = await this.snapshot();
    const pids = (procs) => this.config.processes.flatMap((s) => instancesOf(procs, s, this.bundle).map((p) => `${s.name}:${p.pid}`)).sort();
    const r = await this.start();
    const after = await this.observe({ running: true, loginService: this.loginOn ?? false });
    const same = JSON.stringify(pids(before)) === JSON.stringify(pids(after.snaps.at(-1)));
    const checks = [{ ok: r.code === 0, detail: `open exit ${r.code}` }, ...after.checks, ...judgeDistinct(this.trace, this.config, this.bundle), { ok: same, detail: `same processes as before (${pids(before).join(', ')}) vs after (${pids(after.snaps.at(-1)).join(', ')})` }];
    const ok = checks.every((c) => c.ok);
    return { title, status: ok ? 'pass' : 'fail', summary: ok ? 'the running instance took the launch; no second app or helper' : checks.filter((c) => !c.ok).map((c) => c.detail).join('; ').slice(0, 200), checks, evidence: describeTree(after.snaps.at(-1), this.config, this.bundle) };
  }

  /** Launch at login on, off, on: after each switch the observation window sees no duplicate and only legal parents. */
  async loginHandover() {
    const title = 'Launch at login on, off, on';
    const svc = this.config.loginService;
    if (!svc?.on) return { title, status: 'skip', summary: `no scripted toggle: ${svc?.manualReason ?? 'the app registers its login item itself'}. Manual row: Settings > Launch at login, on, off, on, then count the helper processes` };
    const run = (argv) => this.g.exec(`${sq(`${this.bundle}/${argv[0]}`)} ${argv.slice(1).map(sq).join(' ')} 2>&1`, { timeoutMs: 60_000 });
    const checks = [];
    const evidence = [];
    const loaded = [];
    const phase = async (label, argv, loginService) => {
      this.loginOn = loginService;
      const r = await run(argv);
      checks.push({ ok: r.code === 0, detail: `${label}: \`${argv.slice(1).join(' ')}\` exit ${r.code}: ${r.stdout.trim().slice(0, 120)}` });
      const ready = await this.until(() => this.isReady(), this.config.readyTimeoutSec * 1000);
      const w = await this.observe({ running: true, loginService });
      const lc = parseLaunchctlPrint((await this.g.exec(`launchctl print gui/$(id -u)/${svc.label} 2>&1`)).stdout);
      checks.push({ ok: ready.ok, detail: `${label}: ready again after ${(ready.tookMs / 1000).toFixed(0)} s` }, ...w.checks.map((c) => ({ ...c, detail: `${label}: ${c.detail}` })));
      if (loginService) loaded.push(lc.state !== null);
      evidence.push(`-- ${label}: launchd job ${svc.label} state=${lc.state} pid=${lc.pid}`, ...describeTree(w.snaps.at(-1), this.config, this.bundle));
    };
    await this.reset();
    this.loginOn = false;
    await this.start();
    await this.until(() => this.isReady(), this.config.readyTimeoutSec * 1000);
    await phase('on', svc.on, true);
    await phase('off', svc.off, false);
    await phase('on again', svc.on, true);
    await run(svc.off);
    const ok = checks.every((c) => c.ok);
    // Duplicates are judged either way, but a handover that never registered the launchd job is not proven: say so instead of PASS.
    const proven = loaded.every(Boolean);
    if (ok && !proven) checks.push({ ok: true, detail: `launchd never loaded ${svc.label} after "on" (launchctl print found no job): the CLI route did not register it, so the handover itself was not exercised` });
    const status = !ok ? 'fail' : proven ? 'pass' : 'skip';
    const summary = !ok ? checks.filter((c) => !c.ok).map((c) => c.detail).join('; ').slice(0, 200)
      : proven ? 'no duplicate and no illegal parent after any switch' : 'no duplicate after any switch, but the launchd job never loaded, so the handover is NOT proven: manual row (Settings > Launch at login, three times)';
    return { title, status, summary, checks, evidence };
  }

  /**
   * The update host black-holed: it resolves to loopback (IPv4 and ::1) where a listener accepts every connection, logs it and never
   * answers. A cold start must still reach ready. The step says whether anything connected: a build that never checks for updates on
   * start did not exercise the path, and that is reported as such, not as a pass.
   */
  async updateCheckDoesNotHangStart() {
    const title = 'Start with the update host black-holed';
    const host = this.config.updateHost;
    if (!host) return { title, status: 'skip', summary: 'no update host configured' };
    const tag = '# vm-smoke-blackhole';
    const log = '/tmp/vmsmoke-blackhole.log';
    const perl = [
      'use IO::Socket::IP; use IO::Select;',
      "my @l = grep { $_ } map { IO::Socket::IP->new(LocalAddr => $_, LocalPort => 443, Listen => 16, ReuseAddr => 1) } ('127.0.0.1', '::1');",
      'my $sel = IO::Select->new(@l); my @held;',
      `open(my $log, '>>', '${log}'); select((select($log), $| = 1)[0]);`,
      'print $log "listening " . scalar(@l) . "\\n";',
      'while (1) { for my $s ($sel->can_read(1)) { my $c = $s->accept or next; print $log "contact " . $s->sockhost . "\\n"; push @held, $c; } }',
    ].join('\n');
    const setup = await this.g.exec(`rm -f ${log}; printf %s ${sq(perl)} > /tmp/vmsmoke-blackhole.pl; sudo sh -c ${sq(`printf '127.0.0.1 ${host} ${tag}\\n::1 ${host} ${tag}\\n' >> /etc/hosts; (perl /tmp/vmsmoke-blackhole.pl >/dev/null 2>&1 & echo $!)`)}`);
    const lpid = Number(setup.stdout.trim().split('\n').at(-1));
    let step;
    let contacts = [];
    let listening = '';
    try {
      step = await this.startAndJudge({ title, pre: async () => { await this.g.exec('sudo purge'); } });
      const l = (await this.g.exec(`cat ${log} 2>/dev/null`)).stdout.split('\n').filter(Boolean);
      contacts = l.filter((x) => x.startsWith('contact'));
      listening = l.find((x) => x.startsWith('listening')) ?? '';
    } finally {
      await this.g.exec(`sudo sed -i '' '/${tag}/d' /etc/hosts`);
      if (Number.isInteger(lpid)) await this.g.exec(`ps -p ${lpid} -o command= | grep -q 'vmsmoke-blackhole.pl' && sudo kill ${lpid}`);
    }
    step.checks.unshift({ ok: setup.code === 0 && listening !== '', detail: `${host} points at loopback (IPv4 and ::1) with a listener that accepts and never answers (${listening || 'listener did not start'})` });
    step.checks.push({ ok: true, detail: contacts.length ? `the app connected to the black hole ${contacts.length} time(s) (${[...new Set(contacts)].join(', ')}) and still became ready` : 'nothing connected to the black hole during the start' });
    if (step.status === 'pass' && !contacts.length) {
      step.status = 'skip';
      step.summary = 'NOT EXERCISED: the app never contacted the update host during the start (update checks are off in this build or not on start), so a hung check could not be observed';
    } else if (step.status === 'pass') step.summary += `; contacted the black hole ${contacts.length}x`;
    return step;
  }

  async logs() {
    const evidence = [];
    for (const l of this.config.logs ?? []) {
      const r = await this.g.exec(`tail -n 40 ${sq(await this.path(l))} 2>&1`);
      evidence.push(`--- ${l}`, ...(r.stdout.trim() ? r.stdout.trimEnd().split('\n') : ['(empty or absent)']));
    }
    return { title: 'Logs at the end', status: 'info', summary: 'last 40 lines of each log, for reading', evidence };
  }
}

/** The full sequence. Returns the steps; the caller renders and decides the exit code. */
export async function runAll(check, { stagedDmg, manual }) {
  const steps = [];
  // A failing step is a product FAIL only if the reader could read a state known to pass (the control), else CHECK BROKEN.
  const control = await check.readerControl();
  const push = (s0) => { const s = withReaderControl(s0, control); steps.push(s); check.log(`${s.status.toUpperCase()}  ${s.title}: ${s.summary}`); return s; };
  const install = push(await check.install(stagedDmg));
  if (install.status === 'fail' || install.status === 'broken') return { steps, version: check.version };
  push(await check.startAndJudge({ title: 'Cold start (disk cache dropped)', pre: async () => { await check.g.exec('sudo purge'); } }));
  push(await check.secondLaunch());
  push(await check.quit('Quit with a window open'));
  push(await check.startAndJudge({ title: 'Start without a window (--hidden)', hidden: true }));
  push(await check.quit('Quit with no window'));
  push(await check.startAndJudge({ title: 'Relaunch after Quit' }));
  push(await check.quit('Quit after the relaunch'));
  push(await check.loginHandover());
  push(await check.quit('Quit after the Launch at login switches'));
  push(await check.updateCheckDoesNotHangStart());
  push(await check.quit('Quit after the black-holed start'));
  push(await check.logs());
  for (const m of manual) steps.push({ title: m.title, status: 'manual', summary: m.summary });
  return { steps, version: check.version };
}
