import type { WakeInput, WakeRowState, WakeRouteState, WakeSchedule, WakesView } from '../api.types';

/** Which Schedule states the mock starts in, so every designed state can be seen without the real engine (`?wake=...`). */
export type MockWakeVariant = 'normal' | 'on' | 'off' | 'empty' | 'needsHelper' | 'unavailable' | 'agent' | 'failed';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

/**
 * The mock's scheduled wakes. It mimics the contract with plausible data (the engine is the reference for behaviour): the same
 * validation messages, a row per schedule, the route by variant. It sets no wake and acts at no time.
 */
/** The first local occurrence of a time of day after `after` on a day the repeat allows (the mock's own clock is the browser's). */
export function mockNextAt(minuteOfDay: number, repeat: WakeSchedule['repeat'], after: number): number {
  const start = new Date(after);
  for (let k = 0; k < 9; k++) {
    const c = new Date(start.getFullYear(), start.getMonth(), start.getDate() + k, Math.floor(minuteOfDay / 60), minuteOfDay % 60).getTime();
    const weekday = (new Date(c).getDay() + 6) % 7; // Monday is 0, as the engine counts
    const on = typeof repeat === 'object' ? (repeat.days.mask & (1 << weekday)) !== 0 : true;
    if (on && c > after) return c;
  }
  return after + DAY;
}

/** The latest local occurrence of a time of day at or before `before` on a day the repeat allows. */
export function mockPrevAt(minuteOfDay: number, repeat: WakeSchedule['repeat'], before: number): number {
  const start = new Date(before);
  for (let k = 0; k > -9; k--) {
    const c = new Date(start.getFullYear(), start.getMonth(), start.getDate() + k, Math.floor(minuteOfDay / 60), minuteOfDay % 60).getTime();
    const weekday = (new Date(c).getDay() + 6) % 7;
    const on = typeof repeat === 'object' ? (repeat.days.mask & (1 << weekday)) !== 0 : true;
    if (on && c <= before) return c;
  }
  return before - DAY;
}

export class MockWakes {
  private entries: WakeSchedule[];
  private nextId = 10;

  constructor(
    private readonly variant: MockWakeVariant,
    private readonly now: () => number,
    private readonly isEnabled: () => boolean,
  ) {
    const base = (id: number, name: string, minuteOfDay: number, p: Partial<WakeSchedule>): WakeSchedule => ({
      id,
      name,
      minuteOfDay,
      repeat: 'daily',
      stayAwakeMinutes: null,
      wakeDevices: [],
      job: null,
      enabled: true,
      nextAt: mockNextAt(minuteOfDay, 'daily', now()),
      lastRun: null,
      ...p,
    });
    this.entries =
      variant === 'empty'
        ? []
        : [
            base(1, 'Morning backup check', 7 * 60, { repeat: { days: { mask: 0b0011111 } }, job: 'backupCheck', stayAwakeMinutes: 30, nextAt: mockNextAt(7 * 60, { days: { mask: 0b0011111 } }, now()), lastRun: { at: mockPrevAt(7 * 60, { days: { mask: 0b0011111 } }, now()), summary: 'staying awake 30 min; started its job' } }),
            base(2, 'Wake the NAS for the weekend', 18 * 60, { repeat: { days: { mask: 0b0100000 } }, wakeDevices: [2], nextAt: mockNextAt(18 * 60, { days: { mask: 0b0100000 } }, now()), lastRun: { at: mockPrevAt(18 * 60, { days: { mask: 0b0100000 } }, now()), summary: 'Missed: the computer was off or Dyno was not running then.' } }),
            base(3, 'Once, tomorrow', 6 * 60 + 30, { repeat: 'once', stayAwakeMinutes: 60, enabled: false, nextAt: null }),
          ];
  }

  route(): WakeRouteState {
    switch (this.variant) {
      case 'needsHelper':
        return { needsHelper: { reason: 'Setting a wake needs the administrator helper. Administrator readings are off: turning them on asks for your password once.' } };
      case 'unavailable':
        return { unavailable: { reason: 'Setting a wake needs the administrator helper, and this build has none. The macOS app and the Linux .deb package include it; an AppImage does not.' } };
      case 'agent':
        return {
          agent: {
            note: "Windows wakes the computer for a timer that Dyno sets, so a scheduled wake works while Dyno's agent is running. A computer that is shut down is not woken.",
            cantTell: ["Allow wake timers: Dyno can't tell. Check Power Options, Change advanced settings, Sleep.", "Modern Standby: Dyno can't tell whether wake timers fire on this computer."],
          },
        };
      default:
        return 'helperReady';
    }
  }

  private stateOf(e: WakeSchedule): WakeRowState {
    if (!e.enabled) return 'off';
    if (!this.isEnabled()) return 'paused';
    if (this.variant === 'failed' && e.id === 1) return { failed: { why: 'The helper refused: That time is not in the future.' } };
    const r = this.route();
    if (typeof r === 'object' && 'needsHelper' in r) return 'needsHelper';
    if (typeof r === 'object' && 'unavailable' in r) return { failed: { why: r.unavailable.reason } };
    return { armed: { route: typeof r === 'object' ? 'agent' : 'helper' } };
  }

  view(): WakesView {
    return { rows: this.entries.map((entry) => ({ entry: { ...entry }, state: this.stateOf(entry) })), route: this.route() };
  }

  set(input: WakeInput, knownDevices: number[]): WakesView {
    if (!this.isEnabled()) throw { code: 'badRequest', message: 'Scheduled wake is turned off in Settings.' };
    const name = input.name.trim();
    if (!name) throw { code: 'badRequest', message: 'Give the schedule a name.' };
    if (input.minuteOfDay < 0 || input.minuteOfDay >= 1440) throw { code: 'badRequest', message: 'The time is a time of day, 00:00 to 23:59.' };
    if (typeof input.repeat === 'object' && (input.repeat.days.mask <= 0 || input.repeat.days.mask > 127)) throw { code: 'badRequest', message: 'Choose at least one day.' };
    if (input.stayAwakeMinutes !== null && (input.stayAwakeMinutes < 1 || input.stayAwakeMinutes > 720)) throw { code: 'badRequest', message: 'Stay awake for 1 to 720 minutes.' };
    if (input.wakeDevices.some((d) => !knownDevices.includes(d))) throw { code: 'badRequest', message: 'One of the computers to wake is no longer in the Wake devices list.' };
    if (input.stayAwakeMinutes === null && input.wakeDevices.length === 0 && input.job === null) throw { code: 'badRequest', message: 'Choose what happens after the wake: stay awake, wake a computer or run a job.' };
    const keep = input.id === null ? undefined : this.entries.find((e) => e.id === input.id);
    if (input.id !== null && !keep) throw { code: 'badRequest', message: 'That schedule is no longer in the list.' };
    if (!keep && this.entries.length >= 8) throw { code: 'badRequest', message: 'At most 8 schedules; remove one first.' };
    const next: WakeSchedule = {
      id: keep?.id ?? this.nextId++,
      name,
      minuteOfDay: input.minuteOfDay,
      repeat: input.repeat,
      stayAwakeMinutes: input.stayAwakeMinutes,
      wakeDevices: input.wakeDevices,
      job: input.job,
      enabled: input.enabled,
      nextAt: input.enabled ? mockNextAt(input.minuteOfDay, input.repeat, this.now()) : null,
      lastRun: keep?.lastRun ?? null,
    };
    this.entries = keep ? this.entries.map((e) => (e.id === next.id ? next : e)) : [...this.entries, next];
    return this.view();
  }

  cancel(id: number): WakesView {
    this.entries = this.entries.filter((e) => e.id !== id);
    return this.view();
  }

  enabled(): boolean {
    return this.isEnabled();
  }
}
