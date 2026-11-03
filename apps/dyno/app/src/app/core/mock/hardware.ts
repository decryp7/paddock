import type {
  BaselineItem,
  BaselineOverview,
  BaselineState,
  Confidence,
  Evidence,
  InputStatus,
  Judgement,
  LifeForecast,
  LifespanOverview,
  ThrottleEpisode,
  ThrottleOverview,
  ThrottleReading,
} from '../api.types';
import type { MockOs } from './os';

const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

// Read when the module loads: the router drops the query string soon after.
const OPENED_WITH = typeof location === 'undefined' ? '' : location.search;

/**
 * What the mock hardware says (`?hw=` in the page's address): `calm` (the default: fine now, a few episodes in the past),
 * `hot` (throttled by heat right now), `blind` (this machine reports almost nothing: every card says what is missing),
 * `learning` (baselines still learning).
 */
export type HardwareScenario = 'calm' | 'hot' | 'blind' | 'learning';

export const HARDWARE_SCENARIOS: readonly HardwareScenario[] = ['calm', 'hot', 'blind', 'learning'];

/** The `?hw=` scenario of a query string, or null. */
export function hardwareScenarioFromSearch(search: string): HardwareScenario | null {
  const v = new URLSearchParams(search).get('hw');
  return HARDWARE_SCENARIOS.find((x) => x === v) ?? null;
}

const scenarioNow = (explicit?: HardwareScenario): HardwareScenario => explicit ?? hardwareScenarioFromSearch(OPENED_WITH) ?? 'calm';

const ev = (label: string, value: string): Evidence => ({ label, value });

function episodes(now: number): ThrottleEpisode[] {
  const rows: Omit<ThrottleEpisode, 'id'>[] = [
    { start: now - 1 * DAY - 3 * HOUR, end: now - 1 * DAY - 3 * HOUR + 14 * MIN, cause: 'thermal', avgLostPercent: 34, peakLostPercent: 46, peakTemperatureC: 101 },
    { start: now - 3 * DAY - 5 * HOUR, end: now - 3 * DAY - 5 * HOUR + 38 * MIN, cause: 'thermal', avgLostPercent: 28, peakLostPercent: 41, peakTemperatureC: 99 },
    { start: now - 9 * DAY - 2 * HOUR, end: now - 9 * DAY - 2 * HOUR + 6 * MIN, cause: 'power', avgLostPercent: 22, peakLostPercent: 25, peakTemperatureC: null },
    { start: now - 17 * DAY - 6 * HOUR, end: now - 17 * DAY - 6 * HOUR + 52 * MIN, cause: 'batterySaver', avgLostPercent: 40, peakLostPercent: 40, peakTemperatureC: null },
    { start: now - 24 * DAY - 4 * HOUR, end: now - 24 * DAY - 4 * HOUR + 21 * MIN, cause: 'thermal', avgLostPercent: 31, peakLostPercent: 39, peakTemperatureC: 98 },
  ];
  return rows.map((r) => ({ id: `throttle-${r.start}`, ...r }));
}

function inputs(os: MockOs, scenario: HardwareScenario): InputStatus[] {
  if (scenario === 'blind') {
    return [
      { label: 'CPU clock speed', available: false, detail: "This machine does not report the CPU's clock speed." },
      { label: 'Temperature', available: false, detail: 'This machine does not report a CPU temperature to apps.' },
      { label: "The system's own speed limit", available: false, detail: 'Windows does not report why the CPU slows down; Dyno infers it from the clock, the heat and the power mode.' },
      { label: 'Battery-saving mode', available: false, detail: 'Not known yet.' },
    ];
  }
  const hot = scenario === 'hot';
  return [
    { label: 'CPU clock speed', available: true, detail: hot ? '1.78 GHz of 3.20 GHz' : '3.18 GHz of 3.20 GHz' },
    { label: 'Temperature', available: true, detail: hot ? 'CPU package at 99 °C' : 'CPU package at 71 °C' },
    os === 'windows'
      ? { label: "The system's own speed limit", available: false, detail: 'Windows does not report why the CPU slows down; Dyno infers it from the clock, the heat and the power mode.' }
      : { label: "The system's own speed limit", available: true, detail: hot ? 'Reported: 55% of full speed allowed' : 'Reported: no limit recorded' },
    { label: 'Battery-saving mode', available: true, detail: 'Off' },
  ];
}

/** The Throttling page's data. */
export function mockThrottling(os: MockOs, now: number, scenario?: HardwareScenario): ThrottleOverview {
  const sc = scenarioNow(scenario);
  const past = sc === 'blind' ? [] : episodes(now);
  const total = (e: ThrottleEpisode[]) => Math.round(e.reduce((a, x) => a + ((x.end ?? now) - x.start), 0) / MIN);
  if (sc === 'hot') {
    const current: ThrottleEpisode = { id: `throttle-${now - 7 * MIN}`, start: now - 7 * MIN, end: null, cause: 'thermal', avgLostPercent: 43, peakLostPercent: 45, peakTemperatureC: 101 };
    const reading: ThrottleReading = {
      verdict: { throttled: { cause: 'thermal', lostPercent: 44 } },
      confidence: 'high',
      headline: 'Slowed down by heat: 44% slower',
      explanation:
        'The CPU is giving up about 44% of its speed because the system reports a heat limit. Free the vents, move the machine off soft surfaces, and check for a fan that has stopped or is clogged with dust.',
      evidence: [ev('Load', '96%'), ev('Clock', '1.78 GHz of 4.50 GHz'), ev('Temperature', '99 °C'), ev('Limit set by the system', '55% of full speed')],
    };
    return { at: now, reading, current, episodes: past, minutes30d: total([...past, current]), inputs: inputs(os, sc) };
  }
  if (sc === 'blind') {
    return {
      at: now,
      reading: {
        verdict: 'cannotTell',
        confidence: 'low',
        headline: 'Cannot tell if the CPU is slowed down',
        explanation: "This machine does not report the CPU's clock speed or a speed limit, so Dyno cannot compare it with what it should be.",
        evidence: [ev('Load', '62%')],
      },
      current: null,
      episodes: past,
      minutes30d: 0,
      inputs: inputs(os, sc),
    };
  }
  return {
    at: now,
    reading: {
      verdict: 'fine',
      confidence: 'high',
      headline: 'Running at full speed',
      explanation: 'The CPU is busy and its clock is where it should be.',
      evidence: [ev('Load', '78%'), ev('Clock', '4.40 GHz of 4.50 GHz'), ev('Temperature', '71 °C')],
    },
    current: null,
    episodes: past,
    minutes30d: total(past),
    inputs: inputs(os, sc),
  };
}

const forecast = (f: Omit<LifeForecast, 'restartedAt'> & { restartedAt?: number | null }): LifeForecast => ({ restartedAt: null, ...f });

/** The Lifespan page's data: a laptop has an SSD and a battery, a desktop only the SSD. */
export function mockLifespan(os: MockOs, machine: 'laptop' | 'desktop', scenario?: HardwareScenario): LifespanOverview {
  const sc = scenarioNow(scenario);
  const at = Date.now();
  if (sc === 'blind') {
    const forecasts: LifeForecast[] = [
      forecast({
        subject: 'ssd',
        name: 'SSD',
        outcome: { unavailable: { reason: 'Dyno reads SSD wear through smartmontools, which is not installed. Install it (on macOS: brew install smartmontools).' } },
        confidence: 'low',
        headline: 'Disk life cannot be estimated',
        explanation: 'Dyno reads SSD wear through smartmontools, which is not installed. Install it (on macOS: brew install smartmontools).',
        basis: [],
        historyDays: 0,
        readings: 0,
      }),
    ];
    return { at, forecasts };
  }
  const confidence: Confidence = 'medium';
  const forecasts: LifeForecast[] = [
    forecast({
      subject: 'ssd',
      name: os === 'macos' ? 'APPLE SSD AP1024Z' : os === 'linux' ? 'Samsung SSD 990 PRO 1TB' : 'WD PC SN740 NVMe 1TB',
      outcome: { remaining: { lowDays: 1_900, midDays: 2_700, highDays: 3_650 } },
      confidence,
      headline: 'About 7.4 years of writes left (5.2 to 10 years)',
      explanation: 'Based on the disk’s own wear counter, at the pace of the last 4 months.',
      basis: [ev('Used so far', '9% of rated endurance'), ev('Written so far', '41.3 TB'), ev('Writing', '186 GB/day'), ev('History', '236 readings over 4 months')],
      historyDays: 122,
      readings: 236,
    }),
  ];
  if (machine === 'laptop') {
    forecasts.push(
      forecast({
        subject: 'battery',
        name: 'Battery',
        outcome: { remaining: { lowDays: 520, midDays: 690, highDays: 940 } },
        confidence: 'medium',
        headline: 'Replace in about 23 months (17 to 31 months)',
        explanation: 'Based on its capacity trend over the last 5 months.',
        basis: [ev('Holds', '88% of its original charge'), ev('Charge cycles', '312 of about 1000'), ev('Cycling', '58 cycles a month'), ev('History', '312 readings over 5 months')],
        historyDays: 150,
        readings: 312,
      }),
    );
  }
  if (sc === 'learning') return { at, forecasts: forecasts.map((f) => ({ ...f, outcome: { tooLittleHistory: { haveDays: 6, needDays: f.subject === 'ssd' ? 14 : 30 } }, headline: 'Not enough history for a forecast yet', explanation: `Dyno has 6 days of readings for this ${f.subject === 'ssd' ? 'disk' : 'battery'} and needs about ${f.subject === 'ssd' ? '2 weeks' : '4 weeks'} to see how fast it ${f.subject === 'ssd' ? 'wears' : 'fades'}.`, historyDays: 6, readings: 12 })) };
  return { at, forecasts };
}

const judgement = (state: BaselineState, phrase: string, explanation: string, usual: string, confidence: Confidence = 'high'): Judgement => ({
  state,
  confidence,
  phrase,
  explanation,
  usual,
});

const item = (subject: string, metric: BaselineItem['metric'], label: string, current: string, j: Judgement): BaselineItem => ({ id: `${metric}:${subject}`, subject, metric, label, current, judgement: j });

/** The "Is this normal?" data. */
export function mockBaselines(now: number, scenario?: HardwareScenario): BaselineOverview {
  const sc = scenarioNow(scenario);
  if (sc === 'learning') return { at: now, learning: { learning: { day: 3, daysNeeded: 7, since: now - 2 * 86_400_000 } }, items: [] };
  return {
    at: now,
    learning: { ready: { days: 34 } },
    items: [
      item(
        'System',
        'cpu',
        'CPU',
        '71%',
        judgement({ higher: 'strong' }, 'Higher than usual for Thursday afternoons', 'CPU is 71% now; it is usually about 14% (9% to 22%) on Thursday afternoons, from 5 similar hours.', '14%'),
      ),
      item(
        'rustc',
        'appCpu',
        'CPU',
        '41%',
        judgement({ higher: 'notable' }, 'Higher than usual for Thursday afternoons', 'CPU is 41% now; it is usually about 6% (2% to 12%) on Thursday afternoons, from 5 similar hours.', '6%', 'medium'),
      ),
      item('System', 'memory', 'Memory', '58%', judgement('usual', 'Usual for Thursday afternoons', 'Memory is 58% now; it is usually about 55% (49% to 61%) on Thursday afternoons, from 5 similar hours.', '55%')),
      item('System', 'diskWrite', 'Disk writes', '4.1 MB/s', judgement('usual', 'Usual for Thursday afternoons', 'Disk writes is 4.1 MB/s now; it is usually about 3.4 MB/s (1.9 MB/s to 6.8 MB/s) on Thursday afternoons, from 5 similar hours.', '3.4 MB/s')),
      item('System', 'temperature', 'Temperature', '74 °C', judgement({ higher: 'notable' }, 'Higher than usual for Thursday afternoons', 'Temperature is 74 °C now; it is usually about 58 °C (54 °C to 63 °C) on Thursday afternoons, from 5 similar hours.', '58 °C', 'medium')),
      item('Google Chrome', 'appMemory', 'Memory', '4.3 GB', judgement('usual', 'Usual for weekday afternoons', 'Memory is 4.3 GB now; it is usually about 4.1 GB (3.6 GB to 4.8 GB) on weekday afternoons, from 24 similar hours.', '4.1 GB', 'medium')),
    ],
  };
}
