// The idle action checks (ADR 0032 section 9) on a macOS guest: a one-minute idle rule is seeded in Dyno's settings, then the CLI
// (`dyno idle`) reads the countdown, Cancel and Snooze, and a long sound proves it waits and says why. The VM cannot sleep, so the check
// never lets a countdown reach zero: the OS call Dyno would make (`/usr/bin/pmset sleepnow`) is recorded from its mapping, and the
// agent's own log lines are quoted as evidence.
const sq = (s) => `'${String(s).replace(/'/g, `'\\''`)}'`;
const SETTINGS = '$HOME/Library/Application Support/Dyno/settings.json';

import { makeResult } from './verdict.mjs';

export const idleRule = { enabled: true, action: 'sleep', afterSecs: 60, countdownSecs: 90, acOnly: false, exceptions: { audio: true, otherStayAwake: false, fullScreen: false, cpuPct: null, diskBytesPerS: null, netBytesPerS: null, programs: [] } };

export class MacIdleChecks {
  /** @param power a PowerChecks (for dyno()), @param check the vm-smoke Check (start/quit/reset), @param opts {log} */
  constructor(guest, power, check, { log = () => {}, ui = null, audioDevice = null } = {}) {
    // audioDevice: true/false when the harness knows (tart runs with --no-audio, so false); null = ask the guest.
    Object.assign(this, { g: guest, power, check, log, ui, audioDevice, audioStep: null });
  }

  async seedRule() {
    const py = `import json,os;p=os.path.expandvars(${sq(SETTINGS)});d=json.load(open(p)) if os.path.exists(p) else {};d.setdefault('power',{})['idle']=${JSON.stringify(idleRule).replace(/true/g, 'True').replace(/false/g, 'False').replace(/null/g, 'None')};os.makedirs(os.path.dirname(p),exist_ok=True);json.dump(d,open(p,'w'))`;
    return this.g.exec(`python3 -c ${sq(py)}`);
  }

  /** A real HID event (a tiny pointer drag through the HID event tap), which resets the idle time Dyno reads; `caffeinate -u` does not. */
  async activity() { await this.g.exec('/usr/local/bin/axdrive drag 500 400 505 405'); }

  /** Polls an async condition until it holds or `guardMs` of guest time passes. */
  async poll2(fn, guardMs, intervalMs = 2000) {
    const t0 = this.g.now();
    for (;;) { if (await fn()) return true; if (this.g.now() - t0 >= guardMs) return false; await this.g.sleep(intervalMs); }
  }

  async poll(re, guardMs, intervalMs = 3000) {
    const t0 = this.g.now();
    let last = '';
    for (;;) {
      last = (await this.power.dyno('idle')).stdout;
      if (re.test(last)) return { ok: true, text: last };
      if (this.g.now() - t0 >= guardMs) return { ok: false, text: last };
      await this.g.sleep(intervalMs);
    }
  }

  /** What the guest looked like when the start under the rule failed: the start judgement's failing checks, processes, the agent's launchd job, the agent log tail, the load. */
  async guestState(started) {
    const sh = async (c) => (await this.g.exec(c)).stdout.trimEnd();
    return ['guest state after the failed start:',
      ...(started.checks ?? []).filter((c) => !c.ok).map((c) => `  failed: ${c.detail}`),
      `$ ps: ${(await sh('ps -axo pid,etime,command | grep -E "dyno-(app|agent)" | grep -v grep')) || '(no dyno process)'}`,
      `$ launchctl print gui/$(id -u)/net.decryptology.dyno.agent:\n${(await sh('launchctl print gui/$(id -u)/net.decryptology.dyno.agent 2>&1 | head -12')).replace(/^/gm, '  ')}`,
      `$ agent log tail:\n${(await sh('tail -n 12 "$HOME"/Library/Logs/Dyno/dyno-agent.*.log 2>&1 | cut -c1-260')).replace(/^/gm, '  ')}`,
      `$ uptime: ${await sh('uptime')}`];
  }

  async run() {
    const title = 'Idle action: countdown, Cancel and Snooze';
    const audioTitle = 'Idle action: waits while audio plays';
    const audioChecks = [];
    this.audioStep = null;
    const checks = [];
    const evidence = [];
    await this.check.quit('Quit before seeding the idle rule');
    const seed = await this.seedRule();
    checks.push({ ok: seed.code === 0, detail: `idle rule seeded (1 min idle, 90 s countdown, AC not required): exit ${seed.code}` });
    const started = await this.check.startAndJudge({ title: 'Start with the idle rule' });
    checks.push({ ok: started.status !== 'fail', detail: `Dyno started with the rule: ${started.status}${started.status === 'fail' ? `: ${started.summary}` : ''}` });
    if (started.status === 'fail') evidence.push(...(await this.guestState(started))); // ER-27: a bare "fail" two runs in a row could not be traced

    // 1. The countdown appears, then Cancel stops it.
    const cd = await this.poll(/Counting down to sleep/, 240_000);
    evidence.push('$ dyno idle (countdown)', ...cd.text.trimEnd().split('\n'));
    checks.push({ ok: cd.ok, detail: cd.ok ? 'the countdown appeared after the idle time' : `no countdown within 240 s (last: ${cd.text.trim().split('\n')[0]})` });
    let cancel;
    if (this.ui) {
      const shown = (await this.ui.openPower('Sleep and shut down')) && (await this.ui.waitFor(() => this.ui.headingMatches(/^Will sleep in/), 60_000)).ok;
      checks.push({ ok: shown, detail: shown ? 'the Power page shows the countdown ("Will sleep in ...")' : 'the Power page showed no "Will sleep in" countdown' });
      evidence.push(`screenshot: ${await this.ui.shot('idle-countdown')}`);
      const pressed = await this.ui.press(this.ui.app, '^Cancel sleep$');
      checks.push({ ok: pressed, detail: pressed ? 'pressed "Cancel sleep"' : 'no "Cancel sleep" button' });
      cancel = { code: pressed ? 0 : 1 };
    } else {
      cancel = await this.power.dyno('idle cancel');
      evidence.push(`$ dyno idle cancel (exit ${cancel.code})`, ...cancel.stdout.trimEnd().split('\n'));
    }
    const stopped = await this.poll(/will not try again|Watching/, 20_000, 1500);
    checks.push({ ok: cancel.code === 0 && stopped.ok && !/Counting down/.test(stopped.text), detail: `Cancel stopped the countdown: ${stopped.text.trim().split('\n')[0]}` });

    // 2. New activity re-arms the rule; a second countdown is Snoozed.
    await this.activity();
    const cd2 = await this.poll(/Counting down to sleep/, 240_000);
    checks.push({ ok: cd2.ok, detail: cd2.ok ? 'after new activity the countdown came again' : 'no second countdown after activity' });
    let snooze;
    if (this.ui) {
      await this.ui.openPower('Sleep and shut down');
      const pressed = (await this.ui.waitFor(() => this.ui.press(this.ui.app, '^Snooze 30 minutes$'), 30_000)).ok;
      checks.push({ ok: pressed, detail: pressed ? 'pressed "Snooze 30 minutes"' : 'no "Snooze 30 minutes" button' });
      snooze = { code: pressed ? 0 : 1 };
      const head = await this.ui.waitFor(() => this.ui.headingMatches(/^Snoozed/), 15_000);
      checks.push({ ok: head.ok, detail: head.ok ? 'the page says "Snoozed"' : 'the page never said "Snoozed"' });
    } else {
      snooze = await this.power.dyno('idle snooze 30');
      evidence.push(`$ dyno idle snooze 30 (exit ${snooze.code})`, ...snooze.stdout.trimEnd().split('\n'));
    }
    const snoozed = await this.poll(/Snoozed/, 20_000, 1500);
    evidence.push(...snoozed.text.trimEnd().split('\n'));
    checks.push({ ok: snooze.code === 0 && snoozed.ok && !/Counting down/.test(snoozed.text), detail: snoozed.ok ? 'Snooze 30 minutes: the rule is watching and snoozed' : 'the rule was not snoozed' });
    await this.power.dyno('idle cancel');

    // 3. Audio: the rule waits and says so. The sound is a long spoken file played by the guest's own audio device. Its own step: with no guest audio
    // device it is "not measured" with the reason (never played through the host, ER-14; never a pass, ER-33), and the other idle checks still count.
    const hasAudio = this.audioDevice ?? (await this.g.exec(`system_profiler SPAudioDataType 2>/dev/null | grep -qiE 'Output Channels|Default Output Device'`)).code === 0;
    if (!hasAudio) {
      this.audioStep = { title: audioTitle, status: 'not-measured', cls: 'check', summary: 'not measured: the guest has no audio device (tart run --no-audio, so the VM cannot play a sound and the host is never used for it)', checks: [{ ok: false, kind: 'check', detail: 'no guest audio device' }], evidence: ['guest audio device: none (tart run --no-audio)'] };
    } else {
      // The sound is rendered BEFORE the activity event that starts the idle clock, so the rendering (39 s on a loaded host) is not part of the 60 s + 90 s window.
      const made = await this.g.exec('[ -s /tmp/vmsmoke-long.aiff ] || ( yes hello | head -400 | say -o /tmp/vmsmoke-long.aiff ); [ -s /tmp/vmsmoke-long.aiff ]', { timeoutMs: 170_000 });
      if (made.code !== 0) audioChecks.push({ ok: false, kind: 'check', detail: `the sound file could not be made (say exit ${made.code}), so "waits while audio plays" was not tested` });
      await this.activity();
      const audioEv = [];
      if (made.code === 0) {
        await this.g.exec('(nohup afplay /tmp/vmsmoke-long.aiff >/dev/null 2>&1 &)');
        // The precondition (ER-33): the OS must show the sound holding a power assertion before the rule is judged on it.
        const holding = await this.poll2(async () => (await this.g.exec('pmset -g assertions | grep -i coreaudiod')).stdout.trim() !== '', 30_000);
        if (!holding) audioChecks.push({ ok: false, kind: 'check', detail: 'the sound never held a power assertion within 30 s (afplay or the guest audio device is not playing), so "waits while audio plays" was not tested' });
        else {
          const wait = await this.poll(/Waiting because .*audio/i, 240_000);
          audioEv.push('$ dyno idle (audio playing)', ...wait.text.trimEnd().split('\n'));
          audioChecks.push({ ok: wait.ok && !/Counting down/.test(wait.text), detail: wait.ok ? 'with a sound playing the rule waits and says: ' + wait.text.match(/Waiting because [^\n]*/)[0] : 'the rule did not report waiting for audio' });
        }
      }
      const audio = await this.g.exec('pmset -g assertions | grep -i coreaudiod');
      audioEv.push('$ pmset -g assertions | grep coreaudiod', audio.stdout.trim() || '(none)');
      await this.g.exec('pkill afplay');
      this.audioStep = makeResult(audioTitle, audioChecks, audioEv, 'with a sound playing the rule waits and says why');
    }

    // 4. The OS call Dyno would have made, and what the agent logged. The VM is not slept.
    const log = await this.g.exec(`grep -ih "sleep\\|idle\\|countdown\\|snooze" "$HOME/Library/Logs/Dyno/agent.log" | tail -20`);
    evidence.push('--- agent log (idle lines)', ...(log.stdout.trim() ? log.stdout.trimEnd().split('\n') : ['(none)']));
    evidence.push('OS call for Sleep on macOS: /usr/bin/pmset sleepnow (apps/dyno/crates/dyno-platform/src/power_actions.rs command_for). Not made: the VM is not slept.');
    const failed = checks.filter((c) => !c.ok);
    return makeResult(title, checks, evidence, 'countdown, Cancel and Snooze worked; no sleep was made (VM)');
  }
}
