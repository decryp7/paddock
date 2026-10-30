import type { Alarm, AlarmChange, CustomAlertRule } from '../../core/api.types';
import { demoAlarms } from '../../core/mock/alarms';
import {
  alarmSuffix,
  changeLabel,
  clockToMinutes,
  defaultAlarmConfig,
  formatHold,
  isShelved,
  mergeAlarms,
  metricInfo,
  minutesToClock,
  needsAcknowledge,
  prependChange,
  ruleSummary,
  severityWord,
  shortTime,
  sortOpen,
  stateTone,
  stateWord,
  uniqueRuleId,
  withQuiet,
} from './alarms-model';

const NOW = 1_700_000_000_000;
const rule = (p: Partial<CustomAlertRule> = {}): CustomAlertRule => ({ id: 'x', name: 'X', metric: 'cpuTotal', op: 'above', limit: 90, forSecs: 120, severity: 'warning', enabled: true, ...p });
const change = (kind: AlarmChange['kind'], alarm: Alarm, at = NOW): AlarmChange => ({ kind, alarm, notify: false, at });

describe('rule summaries', () => {
  it('reads like a sentence with the unit of the metric', () => {
    expect(ruleSummary(rule())).toBe('CPU is above 90% for 2 min');
    expect(ruleSummary(rule({ metric: 'temperature', op: 'below', limit: 5, forSecs: 0 }))).toBe('Temperature is below 5 °C');
    expect(ruleSummary(rule({ forSecs: 90 }))).toBe('CPU is above 90% for 1 min 30 sec');
  });
  it('formats holds', () => {
    expect(formatHold(30)).toBe('30 sec');
    expect(formatHold(3600)).toBe('60 min');
  });
  it('falls back to the first metric', () => {
    expect(metricInfo('gpuLoad').label).toBe('GPU');
    expect(metricInfo('nope' as never).id).toBe('cpuTotal');
  });
});

describe('uniqueRuleId', () => {
  it('slugs the name and avoids taken ids', () => {
    expect(uniqueRuleId('GPU limit!', [])).toBe('gpu-limit');
    expect(uniqueRuleId('GPU limit', ['gpu-limit', 'gpu-limit-2'])).toBe('gpu-limit-3');
    expect(uniqueRuleId('  ***  ', [])).toBe('rule');
    expect(uniqueRuleId('Ünïcode name', [])).toBe('unicode-name');
    expect(uniqueRuleId('a'.repeat(60), []).length).toBe(40);
  });
});

describe('clock conversion', () => {
  it('converts both ways and rejects bad text', () => {
    expect(minutesToClock(0)).toBe('00:00');
    expect(minutesToClock(22 * 60 + 5)).toBe('22:05');
    expect(minutesToClock(5000)).toBe('23:59');
    expect(clockToMinutes('07:30')).toBe(450);
    expect(clockToMinutes('7:30')).toBe(450);
    expect(clockToMinutes('24:00')).toBeNull();
    expect(clockToMinutes('12:61')).toBeNull();
    expect(clockToMinutes('')).toBeNull();
  });
  it('patches the quiet hours only', () => {
    const c = withQuiet(defaultAlarmConfig(), { enabled: true });
    expect(c.quiet.enabled).toBe(true);
    expect(c.quiet.startMinute).toBe(1320);
    expect(c.chatterTransitions).toBe(defaultAlarmConfig().chatterTransitions);
  });
});

describe('words', () => {
  it('names severities, states and changes in sentence case', () => {
    expect(severityWord('critical')).toBe('Critical');
    expect(severityWord('warning')).toBe('Warning');
    expect(severityWord('info')).toBe('Info');
    expect(stateWord('active')).toBe('Active');
    expect(stateWord('acknowledged')).toBe('Acknowledged');
    expect(stateWord('clearedUnacknowledged')).toBe('Returned, not acknowledged');
    expect(stateWord('cleared')).toBe('Cleared');
    expect(stateTone('active')).toBe('bad');
    expect(stateTone('clearedUnacknowledged')).toBe('warn');
    expect(stateTone('acknowledged')).toBe('calm');
    const a = demoAlarms(NOW)[0]!;
    const kinds: AlarmChange['kind'][] = ['raised', 'repeated', 'returned', 'acknowledged', 'shelved', 'unshelved', 'noted', 'chattering', 'pruned'];
    expect(kinds.map((k) => changeLabel(change(k, a)))).toEqual(['Raised', 'Repeated', 'Returned to normal', 'Acknowledged', 'Shelved', 'Unshelved', 'Note added', 'Started chattering', 'Removed']);
    expect(changeLabel(change('muted', { ...a, muted: true }))).toBe('Rule muted');
    expect(changeLabel(change('muted', a))).toBe('Rule unmuted');
  });
});

describe('alarm helpers', () => {
  const alarms = demoAlarms(NOW);
  const by = (id: number) => alarms.find((a) => a.id === id)!;
  it('knows what needs acknowledging', () => {
    expect(needsAcknowledge(by(7))).toBe(true);
    expect(needsAcknowledge(by(4))).toBe(true);
    expect(needsAcknowledge(by(5))).toBe(false);
  });
  it('knows when a shelf is in force', () => {
    expect(isShelved(null, NOW)).toBe(false);
    expect(isShelved('untilReboot', NOW)).toBe(true);
    expect(isShelved({ until: NOW + 1 }, NOW)).toBe(true);
    expect(isShelved({ until: NOW - 1 }, NOW)).toBe(false);
  });
  it('writes one muted suffix', () => {
    expect(alarmSuffix(by(7), NOW)).toBe('');
    expect(alarmSuffix({ ...by(7), shelved: 'untilReboot', muted: true, chattering: true }, NOW)).toBe('Shelved until restart · Muted · Chattering');
    expect(alarmSuffix(by(3), NOW)).toMatch(/^Shelved until \d\d:\d\d$/);
    expect(alarmSuffix({ ...by(7), shelved: { until: NOW - 5 } }, NOW)).toBe('');
  });
  it('shows a date for other days only', () => {
    expect(shortTime(NOW, NOW)).toMatch(/^\d\d:\d\d$/);
    expect(shortTime(NOW - 3 * 86_400_000, NOW)).toMatch(/, \d\d:\d\d$/);
  });
  it('sorts newest first, merges changes and drops cleared alarms', () => {
    const open = sortOpen(alarms.filter((a) => a.state !== 'cleared'));
    expect(open[0]!.id).toBe(6);
    const merged = mergeAlarms(open, [{ ...by(7), state: 'acknowledged' }, { ...by(6), state: 'cleared' }]);
    expect(merged.find((a) => a.id === 7)?.state).toBe('acknowledged');
    expect(merged.find((a) => a.id === 6)).toBeUndefined();
    expect(mergeAlarms([], [{ ...by(7), id: 99 }])).toHaveLength(1);
  });
  it('prepends a change once and keeps the log capped and ordered', () => {
    const c = change('noted', by(7), NOW);
    const log = prependChange([], c, 2);
    expect(prependChange(log, c, 2)).toHaveLength(1);
    const more = prependChange(prependChange(log, change('raised', by(7), NOW - 10), 2), change('raised', by(6), NOW - 20), 2);
    expect(more.map((x) => x.at)).toEqual([NOW, NOW - 10]);
  });
});
