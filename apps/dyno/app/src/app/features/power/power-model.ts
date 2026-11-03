import type { WakeSettings, WolSettings, BusyNow, IdleAction, IdleView, PowerSettings, TimedAction, AwakeSpec, AwakeUntil, AwakeView, BusyCond, Capability, KeepAwakeSettings, PowerView } from '../../core/api.types';

/** The quick choices, in the order the page and the tray offer them. */
export const AWAKE_PRESETS: readonly { id: string; label: string; until: AwakeUntil }[] = [
  { id: '30m', label: '30 minutes', until: { for: { minutes: 30 } } },
  { id: '1h', label: '1 hour', until: { for: { minutes: 60 } } },
  { id: '2h', label: '2 hours', until: { for: { minutes: 120 } } },
  { id: '4h', label: '4 hours', until: { for: { minutes: 240 } } },
  { id: 'indefinite', label: 'Until I stop it', until: 'indefinite' },
];

export type AwakeKind = 'off' | 'holding' | 'waiting' | 'ended';
type Holding = Extract<AwakeView, { holding: unknown }>['holding'];
type Waiting = Extract<AwakeView, { waiting: unknown }>['waiting'];
type Ended = Extract<AwakeView, { ended: unknown }>['ended'];

/** Which state the engine's externally tagged view is in. */
export function awakeKind(a: AwakeView): AwakeKind {
  if (a === 'off') return 'off';
  return 'holding' in a ? 'holding' : 'waiting' in a ? 'waiting' : 'ended';
}
export const holdingOf = (a: AwakeView): Holding | null => (typeof a === 'object' && 'holding' in a ? a.holding : null);
export const waitingOf = (a: AwakeView): Waiting | null => (typeof a === 'object' && 'waiting' in a ? a.waiting : null);
export const endedOf = (a: AwakeView): Ended | null => (typeof a === 'object' && 'ended' in a ? a.ended : null);

/** The reason a feature cannot be used, when the engine said it cannot tell. */
export function cantTellReason(c: Capability): string | null {
  return c === 'known' ? null : c.cantTell.reason;
}

/** The conditions of a "while busy" hold the person ticked, with the thresholds from Settings. */
export function busyConditions(picked: { cpu: boolean; network: boolean; disk: boolean }, s: KeepAwakeSettings): BusyCond[] {
  const conds: BusyCond[] = [];
  if (picked.cpu) conds.push({ cpu: { abovePct: s.cpuPct } });
  if (picked.network) conds.push({ network: { aboveBytesPerS: s.netBytesPerS } });
  if (picked.disk) conds.push({ disk: { aboveBytesPerS: s.diskBytesPerS } });
  return conds;
}

/** "HH:MM" from a time input as minutes after midnight, or null when it is not a time. */
export function minuteOfDay(text: string): number | null {
  const m = /^(\d{1,2}):(\d{2})$/.exec(text.trim());
  if (!m) return null;
  const [h, min] = [Number(m[1]), Number(m[2])];
  return h < 24 && min < 60 ? h * 60 + min : null;
}

/** A duration in plain words: "42 min", "1 h 5 min", "2 h", "3 d". Under a minute: "less than a minute". */
export function durationText(ms: number): string {
  const minutes = Math.floor(Math.max(0, ms) / 60_000);
  if (minutes < 1) return 'less than a minute';
  const [d, h, m] = [Math.floor(minutes / 1440), Math.floor((minutes % 1440) / 60), minutes % 60];
  if (d > 0) return h > 0 ? `${d} d ${h} h` : `${d} d`;
  if (h > 0) return m > 0 ? `${h} h ${m} min` : `${h} h`;
  return `${m} min`;
}

/** What a hold waits for, in words ("for 1 hour", "until you stop it", "while a program you chose runs"). */
export function untilText(until: AwakeUntil): string {
  if (until === 'indefinite') return 'until you stop it';
  if ('for' in until) return `for ${durationText(until.for.minutes * 60_000)}`;
  if ('at' in until) {
    const m = until.at.minuteOfDay;
    return `until ${String(Math.floor(m / 60)).padStart(2, '0')}:${String(m % 60).padStart(2, '0')}`;
  }
  if ('whileProcess' in until) return 'while the program you chose runs';
  return `while the computer is busy (${until.whileBusy.conds.map(condText).join(', ')}), and ${durationText(until.whileBusy.graceSecs * 1000)} after`;
}

function condText(c: BusyCond): string {
  if ('cpu' in c) return `CPU above ${c.cpu.abovePct}%`;
  if ('network' in c) return `network above ${Math.round(c.network.aboveBytesPerS / 1024)} KB/s`;
  if ('disk' in c) return `disk above ${Math.round(c.disk.aboveBytesPerS / 1024 / 1024)} MB/s`;
  return `${c.process.program} running`;
}

/** The headline for the state: always words, never colour alone. A paused hold says only "Paused". */
export function headline(v: AwakeView, now: number): string {
  const h = holdingOf(v);
  if (h?.busy) return h.busy.active ? `Keeping awake: ${h.busy.detail}` : 'Waiting for the computer to get busy';
  if (h) return h.ends === null ? `Keeping awake ${startedByText(h.spec.until)}` : `Keeping awake, ${durationText(h.ends - now)} left`;
  if (waitingOf(v)) return 'Paused';
  const e = endedOf(v);
  return e ? `Keep awake ended: ${e.why}` : 'Not keeping your computer awake';
}

/** The tray's header line when a hold is active, or null. */
export function trayHeader(v: PowerView, now: number): string | null {
  const h = holdingOf(v.awake);
  if (h) return h.ends === null ? 'Keeping awake' : `Keeping awake, ${durationText(h.ends - now)} left`;
  if (waitingOf(v.awake)) return 'Keep awake paused: asleep';
  const e = endedOf(v.awake);
  return e ? `Keep awake ended: ${e.why}` : null;
}

/** Builds the request for a preset. `display` is the person's choice (the setting is its default). */
export function specFor(until: AwakeUntil, display: boolean): AwakeSpec {
  return { scope: display ? 'systemAndDisplay' : 'system', until };
}

/** Why the options cannot be used right now, or null. */
export function unavailableReason(v: PowerView | null, enabled: boolean): string | null {
  if (!enabled) return 'Keep awake is turned off in Settings.';
  const why = v ? cantTellReason(v.caps.keepAwake) : null;
  return why ? `Not available here: ${why}.` : null;
}

/** The keep-awake settings before the agent has answered (the same values as the engine's defaults). */
export const DEFAULT_KEEP_AWAKE: KeepAwakeSettings = {
  enabled: true,
  display: false,
  graceSecs: 120,
  cpuPct: 50,
  netBytesPerS: 512_000,
  diskBytesPerS: 5_242_880,
  trayItem: false,
};

/** The Wake-on-LAN settings before the agent has answered (the same values as the engine's defaults). */
export const DEFAULT_WOL: WolSettings = { enabled: true, discovery: true, directedBroadcast: true, testTimeoutSecs: 120 };

/** The scheduled-wake settings before the agent has answered (off, like the engine's default). */
export const DEFAULT_WAKE: WakeSettings = { enabled: false };

/** The power settings before the agent has answered (the engine's defaults). */
export const DEFAULT_POWER: PowerSettings = {
  keepAwake: DEFAULT_KEEP_AWAKE,
  energy: { enabled: true, wattsOn: null, wattsSleep: null, tariffPerKwh: null, currency: null, co2GPerKwh: null },
  idle: {
    enabled: false,
    action: 'sleep',
    afterSecs: 1800,
    countdownSecs: 60,
    acOnly: true,
    exceptions: { audio: true, fullScreen: true, otherStayAwake: true, cpuPct: 50, diskBytesPerS: 5_242_880, netBytesPerS: 512_000, programs: [] },
  },
  timed: { enabled: false, entries: [] },
  wol: DEFAULT_WOL,
  wake: DEFAULT_WAKE,
};

/** "sleep", "hibernate", "shut down". */
export function actionVerb(a: IdleAction): string {
  return a === 'shutDown' ? 'shut down' : a;
}

/** The copy that says what the action does and what Dyno cannot do (shown before turning one on). */
export function actionWarning(a: IdleAction): string {
  if (a === 'shutDown') return "Dyno asks your computer to shut down the normal way. Apps with unsaved work may stop it and ask you. Dyno can't save other apps' work.";
  if (a === 'hibernate') return 'Dyno asks your computer to hibernate the normal way. Apps may still be running when it wakes; nothing is closed.';
  return 'Dyno asks your computer to sleep the normal way. Nothing is closed; it wakes when you use it.';
}

export type IdleKind = 'disabled' | 'unavailable' | 'watching' | 'countdown' | 'acting' | 'stopped';

/** Which state the engine's externally tagged idle view is in. */
export function idleKind(v: IdleView): IdleKind {
  if (v === 'disabled') return 'disabled';
  if ('unavailable' in v) return 'unavailable';
  if ('watching' in v) return 'watching';
  if ('countdown' in v) return 'countdown';
  if ('acting' in v) return 'acting';
  return 'stopped';
}

/** The headline for the idle state, in words, with the seconds left in a countdown. */
export function idleHeadline(v: IdleView, now: number): string {
  if (v === 'disabled') return 'Sleep and shut down rules are off';
  if ('unavailable' in v) return 'The idle rule cannot work on this computer';
  if ('watching' in v) return v.watching.snoozedUntil !== null ? 'Snoozed' : v.watching.blockers.length > 0 ? 'Waiting: something is in the way' : `Watching: idle for ${durationText(v.watching.idleSecs * 1000)}`;
  if ('countdown' in v) {
    const secs = Math.max(0, Math.round((v.countdown.deadline - now) / 1000));
    return `Will ${actionVerb(v.countdown.action)} in ${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, '0')}`;
  }
  if ('acting' in v) return `Waiting for the system to ${actionVerb(v.acting.action)}`;
  return v.stopped.why;
}

/** Local "HH:MM" for a minute of the day. */
export function clockOf(minute: number): string {
  return `${String(Math.floor(minute / 60)).padStart(2, '0')}:${String(minute % 60).padStart(2, '0')}`;
}

/** "Every day", "Once", "Mon, Wed" for a repeat. */
export function repeatText(r: TimedAction['repeat']): string {
  if (r === 'once') return 'Once';
  if (r === 'daily') return 'Every day';
  const names = ['Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat', 'Sun'];
  return names.filter((_, i) => (r.weekdays & (1 << i)) !== 0).join(', ');
}

/** "1 hour", "30 minutes", "1 hour 30 minutes", "2 days": a length in minutes in plain words. */
export function longDuration(minutes: number): string {
  const [d, h, m] = [Math.floor(minutes / 1440), Math.floor((minutes % 1440) / 60), minutes % 60];
  const part = (n: number, one: string, many: string): string => `${n} ${n === 1 ? one : many}`;
  return [d > 0 ? part(d, 'day', 'days') : '', h > 0 ? part(h, 'hour', 'hours') : '', m > 0 ? part(m, 'minute', 'minutes') : ''].filter(Boolean).join(' ');
}

/** What a hold waits for, as a phrase after "you": "for 1 hour", "until 18:30", "until you stop it", "while the computer is busy". */
export function startedByText(until: AwakeUntil): string {
  if (typeof until === 'object' && 'for' in until) return `for ${longDuration(until.for.minutes)}`;
  if (typeof until === 'object' && 'whileBusy' in until) return 'while the computer is busy';
  return untilText(until);
}

/** What stays awake, in words. */
export function keepsAwakeText(scope: AwakeSpec['scope']): string {
  return scope === 'systemAndDisplay' ? 'Computer and display' : 'Computer (the display may turn off)';
}

/** The conditions of a "while busy" hold: "CPU is above 50% or network is above 500 KB/s". */
export function busyConditionsText(until: AwakeUntil): string | null {
  if (typeof until !== 'object' || !('whileBusy' in until)) return null;
  const parts = until.whileBusy.conds.map((c) =>
    'cpu' in c ? `CPU is above ${c.cpu.abovePct}%` : 'network' in c ? `network is above ${Math.round(c.network.aboveBytesPerS / 1024)} KB/s` : 'disk' in c ? `disk is above ${Math.round(c.disk.aboveBytesPerS / 1048576)} MB/s` : `${c.process.program} is running`,
  );
  return parts.join(' or ');
}

/** The condition sentence of a "while busy" hold: "Waiting: keeps awake when CPU is above 50% or network is above 500 KB/s". */
export function busyWaitingText(until: AwakeUntil): string | null {
  const c = busyConditionsText(until);
  return c === null ? null : `Waiting: keeps awake when ${c}`;
}

/** The label of the primary button for a preset. */
export function presetButtonText(p: { id: string; label: string }): string {
  return p.id === 'indefinite' ? 'Keep awake until I stop it' : `Keep awake for ${p.label}`;
}

/** "Keep awake for 1 hour again": the label of the button on an ended hold. */
export function againText(spec: AwakeSpec | null): string {
  const until = spec?.until ?? { for: { minutes: 60 } };
  return until === 'indefinite' ? 'Keep awake until I stop it again' : `Keep awake ${typeof until === 'object' && 'for' in until ? `for ${longDuration(until.for.minutes)}` : untilText(until)} again`;
}

/** Why a start button is disabled, in words next to it (never a silent grey button). */
export function whyDisabled(kind: 'time' | 'busy' | 'program', ready: boolean, blocked: string | null): string | null {
  if (blocked) return blocked;
  if (ready) return null;
  return kind === 'time' ? 'Pick a time first' : kind === 'busy' ? 'Tick at least one condition first' : 'Choose a program first';
}

/** "1:20" for a number of seconds. */
export function minutesSeconds(secs: number): string {
  return `${Math.floor(secs / 60)}:${String(secs % 60).padStart(2, '0')}`;
}

/** The sentence under a busy hold's headline: why it holds now, or how long until it lets go. */
export function busyStatusLine(b: BusyNow | null): string | null {
  if (!b) return null;
  return b.active ? 'Holds while any of the conditions below hold.' : `Quiet for ${minutesSeconds(b.quietSecs)}; it lets go in ${minutesSeconds(b.releaseInSecs ?? 0)} unless the computer gets busy again.`;
}

/** The idle rule in one line: "Sleep after 30 min idle, 23 min to go" while watching and not yet idle long enough. */
export function idleRuleLine(action: IdleAction, afterSecs: number, idleSecs: number): string {
  const verb = actionVerb(action);
  const rule = `${verb.charAt(0).toUpperCase()}${verb.slice(1)} after ${durationText(afterSecs * 1000)} idle`;
  const left = afterSecs - idleSecs;
  return left > 0 ? `${rule}, ${durationText(left * 1000)} to go` : `${rule}: idle long enough`;
}
