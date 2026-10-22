import type { Schedule, SchedulePreview, Weekday } from '../api.types';

const DAYS: Weekday[] = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun'];
const HOUR = 3_600_000;
const DAY = 24 * HOUR;

/** The mock's stand-in for the engine's schedule description (the real one is computed in Rust). */
export function mockDescribe(s: Schedule): string {
  if (s.mode === 'every') {
    const unit = (n: number, one: string, many: string) => (n === 1 ? one : `${n} ${many}`);
    const secs = s.secs;
    const text = secs % 86_400 === 0 ? unit(secs / 86_400, 'day', 'days') : secs % 3600 === 0 ? unit(secs / 3600, 'hour', 'hours') : secs % 60 === 0 ? unit(secs / 60, 'minute', 'minutes') : unit(secs, 'second', 'seconds');
    return `Every ${text}`;
  }
  const zone = s.zone ? ` (${s.zone})` : '';
  if (s.mode === 'cron') return `Cron ${s.expr.trim()}${zone}`;
  const days = DAYS.filter((d) => s.days.includes(d));
  const label = days.length === 7 ? 'Every day' : days.join(',') === 'mon,tue,wed,thu,fri' ? 'Weekdays' : days.join(',') === 'sat,sun' ? 'Weekends' : days.map((d) => d[0]!.toUpperCase() + d.slice(1)).join(', ');
  return `${label} at ${[...s.times].sort().join(', ')}${zone}`;
}

/** What the engine would refuse, in the engine's words (approximate). */
export function mockScheduleProblems(s: Schedule): string[] {
  if (s.mode === 'times') {
    const out: string[] = [];
    if (!s.days.length) out.push('Pick at least one day.');
    if (!s.times.length) out.push('Add at least one time of day.');
    if (s.zone && !/^[A-Za-z]+(\/[A-Za-z_+-]+)*$/.test(s.zone.trim())) out.push(`Unknown time zone "${s.zone.trim()}". Use an IANA name such as Europe/Berlin or America/New_York.`);
    return out;
  }
  if (s.mode === 'cron') {
    const fields = s.expr.trim().split(/\s+/).filter(Boolean).length;
    if (!fields) return ['Enter a cron expression with five fields, for example 0 9 * * 1-5.'];
    if (fields < 5 || fields > 6) return [`A cron expression has five fields (minute hour day-of-month month day-of-week), but this has ${fields}. Example: 0 9 * * 1-5.`];
  }
  return [];
}

/** A plausible preview: the next five runs (hourly for cron, the chosen times for "at times"). */
export function mockSchedulePreview(s: Schedule, now: number): SchedulePreview {
  const warnings: string[] = [];
  let runs: number[] = [];
  if (s.mode === 'every') {
    const each = Math.max(s.secs, 60);
    if (s.secs < 60) warnings.push('This source is checked at most every minute; that is used instead.');
    runs = [1, 2, 3, 4, 5].map((i) => now + each * 1000 * i);
  } else if (s.mode === 'cron') {
    const next = Math.ceil((now + 1) / HOUR) * HOUR;
    runs = [0, 1, 2, 3, 4].map((i) => next + i * HOUR);
    if (/^\*\s/.test(s.expr.trim())) warnings.push('This runs as often as every minute, more often than this source is checked. It is clamped to at most once every 5 minutes, and the extra times are skipped.');
  } else {
    const start = Math.floor(now / DAY) * DAY;
    for (let d = 0; d < 14 && runs.length < 5; d++) {
      const at = start + d * DAY;
      const weekday = DAYS[(new Date(at).getUTCDay() + 6) % 7]!;
      if (!s.days.includes(weekday)) continue;
      for (const t of [...s.times].sort()) {
        const [h = 0, m = 0] = t.split(':').map(Number);
        const when = at + h * HOUR + m * 60_000;
        if (when > now && runs.length < 5) runs.push(when);
      }
    }
  }
  return { description: mockDescribe(s), nextRunsMs: runs, warnings, zone: s.mode === 'every' ? 'UTC' : (s.zone?.trim() || 'UTC') };
}
