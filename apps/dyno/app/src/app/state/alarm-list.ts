import type { Alarm, AlarmChange } from '../core/api.types';

// The open-list helpers the AlarmsStore needs; they live with the state, not in a feature folder.

export function needsAcknowledge(a: Alarm): boolean {
  return a.state === 'active' || a.state === 'clearedUnacknowledged';
}

/** Open alarms, most recently seen first. */
export function sortOpen(alarms: readonly Alarm[]): Alarm[] {
  return [...alarms].sort((a, b) => b.lastSeen - a.lastSeen || b.id - a.id);
}

/** Replaces (or adds) the changed alarms; cleared ones leave the open list. */
export function mergeAlarms(open: readonly Alarm[], changed: readonly Alarm[]): Alarm[] {
  const ids = new Set(changed.map((a) => a.id));
  return sortOpen([...open.filter((a) => !ids.has(a.id)), ...changed.filter((a) => a.state !== 'cleared')]);
}

/** Puts a change on top of a newest-first log unless the same change is already there. */
export function prependChange(log: readonly AlarmChange[], c: AlarmChange, limit: number): AlarmChange[] {
  if (log.some((x) => x.kind === c.kind && x.at === c.at && x.alarm.id === c.alarm.id)) return [...log];
  return [c, ...log].sort((a, b) => b.at - a.at).slice(0, limit);
}
