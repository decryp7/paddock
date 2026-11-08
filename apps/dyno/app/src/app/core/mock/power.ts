import type { AwakeSpec, AwakeView, EnergyDay, EnergyReport, EnergySettings, IdleView, PowerView } from '../api.types';

/** Which states the mock starts in, so every designed state can be seen without the real engine (`?power=...`). */
export type MockPowerVariant = 'normal' | 'unavailable' | 'ended' | 'waiting' | 'busy' | 'quiet';

/** Which idle state the mock starts in (`?idle=...`). */
export type MockIdleVariant = 'disabled' | 'watching' | 'blocked' | 'countdown' | 'acting' | 'stopped' | 'unavailable';

const MIN = 60_000;

/**
 * The mock's keep awake. It mimics the contract with plausible data (the engine is the reference for behaviour): a start holds,
 * a stop releases, a `for` hold gets an end time. It never ends a hold by itself.
 */
export class MockPower {
  private awake: AwakeView;
  private idle: IdleView;

  constructor(
    private readonly variant: MockPowerVariant,
    private readonly now: () => number,
    idleVariant: MockIdleVariant = 'disabled',
  ) {
    this.idle = mockIdle(idleVariant, now());
    this.awake =
      variant === 'ended'
        ? { ended: { why: 'Dyno restarted', at: now() - 5 * MIN } }
        : variant === 'waiting'
          ? { waiting: { spec: { scope: 'system', until: { for: { minutes: 60 } } }, why: 'the computer went to sleep', since: now() - 20 * MIN, ends: now() + 40 * MIN } }
          : variant === 'busy' || variant === 'quiet'
            ? {
                holding: {
                  spec: { scope: 'system', until: { whileBusy: { conds: [{ cpu: { abovePct: 50 } }, { network: { aboveBytesPerS: 512_000 } }], graceSecs: 120 } } },
                  since: now() - 35 * MIN,
                  ends: null,
                  osRef: 'mock',
                  busy:
                    variant === 'busy'
                      ? { active: true, detail: 'CPU at 72%', quietSecs: 0, releaseInSecs: null }
                      : { active: false, detail: 'quiet', quietSecs: 40, releaseInSecs: 80 },
                },
              }
            : 'off';
  }

  view(): PowerView {
    return {
      awake: this.awake,
      idle: this.idle,
      caps: {
        keepAwake: this.variant === 'unavailable' ? { cantTell: { reason: 'this system has no logind to ask' } } : 'known',
        sleep: 'known',
        hibernate: this.variant === 'unavailable' ? { cantTell: { reason: 'this system decides by itself when to hibernate' } } : 'known',
        shutDown: 'known',
      },
    };
  }

  /** Cancel: the countdown (or the wait after the request) ends and the rule watches again. */
  idleCancel(): PowerView {
    if (typeof this.idle === 'object' && ('countdown' in this.idle || 'acting' in this.idle)) this.idle = { watching: { idleSecs: 1900, blockers: [], snoozedUntil: null } };
    return this.view();
  }

  idleSnooze(minutes: number): PowerView {
    if (typeof this.idle === 'object' && 'countdown' in this.idle) this.idle = { watching: { idleSecs: 1900, blockers: [], snoozedUntil: this.now() + minutes * MIN } };
    return this.view();
  }

  start(spec: AwakeSpec): PowerView {
    if (this.variant === 'unavailable') {
      this.awake = { ended: { why: 'Keep awake is not available here: this system has no logind to ask', at: this.now() } };
      return this.view();
    }
    const since = this.now();
    const ends = typeof spec.until === 'object' && 'for' in spec.until ? since + spec.until.for.minutes * MIN : null;
    this.awake = { holding: { spec, since, ends, osRef: 'mock', busy: null } };
    return this.view();
  }

  stop(): PowerView {
    this.awake = 'off';
    return this.view();
  }
}

const DAY = 86_400_000;

/**
 * A plausible energy report for the mock: a week of mostly-on days with a night asleep, one day with a gap. The engine does the
 * real arithmetic; the mock only prices what the person's numbers say, so the page's states can be seen.
 */
export function mockEnergy(days: number, now: number, s: EnergySettings): EnergyReport {
  const count = Math.max(1, Math.min(days, 366));
  const midnight = new Date(now);
  midnight.setHours(0, 0, 0, 0);
  const rows: EnergyDay[] = [];
  for (let i = count - 1; i >= 0; i--) {
    const start = midnight.getTime() - i * DAY;
    const today = i === 0;
    const gap = i === 3;
    const on = today ? Math.floor((now - start) / 1000) : gap ? 5 * 3600 : 14 * 3600;
    const sleep = today ? 0 : gap ? 0 : 6 * 3600;
    const unknown = today ? 0 : 86_400 - on - sleep;
    rows.push(price({ dayStart: start, onSecs: on, sleepSecs: sleep, unknownSecs: unknown, kwh: null, cost: null, co2Kg: null, cpuKwh: null }, s));
  }
  const sum = (f: (d: EnergyDay) => number) => rows.reduce((a, d) => a + f(d), 0);
  const totals = price({ dayStart: rows[0]!.dayStart, onSecs: sum((d) => d.onSecs), sleepSecs: sum((d) => d.sleepSecs), unknownSecs: sum((d) => d.unknownSecs), kwh: null, cost: null, co2Kg: null, cpuKwh: null }, s);
  return {
    enabled: s.enabled,
    days: rows,
    totals,
    source: s.wattsOn === null ? 'notSet' : 'estimated',
    sleepEnergyCounted: s.wattsSleep !== null,
    currency: s.currency,
    trackingSince: rows[0]!.dayStart,
    cpuVia: 'local',
  };
}

function price(d: EnergyDay, s: EnergySettings): EnergyDay {
  if (s.wattsOn === null) return d;
  const kwh = (d.onSecs * s.wattsOn + d.sleepSecs * (s.wattsSleep ?? 0)) / 3_600_000;
  return { ...d, kwh, cost: s.tariffPerKwh === null ? null : kwh * s.tariffPerKwh, co2Kg: s.co2GPerKwh === null ? null : (kwh * s.co2GPerKwh) / 1000 };
}

/** The mock's idle view for each designed state. */
export function mockIdle(v: MockIdleVariant, now: number): IdleView {
  switch (v) {
    case 'watching':
      return { watching: { idleSecs: 420, blockers: [], snoozedUntil: null } };
    case 'blocked':
      return {
        watching: {
          idleSecs: 2100,
          snoozedUntil: null,
          blockers: [
            { kind: 'audio', reason: 'audio is playing', cantTell: false },
            { kind: 'stayAwake', reason: 'Dyno cannot tell whether another program asked it to stay awake: Windows only lets an administrator list that', cantTell: true },
          ],
        },
      };
    case 'countdown':
      return { countdown: { action: 'sleep', deadline: now + 48_000, cause: 'idle' } };
    case 'acting':
      return { acting: { action: 'shutDown', since: now - 20_000 } };
    case 'stopped':
      return { stopped: { why: 'Shutdown did not happen: an app may be asking you to save' } };
    case 'unavailable':
      return { unavailable: { reason: 'this system gives Dyno no idle time' } };
    default:
      return 'disabled';
  }
}
