import type { PauseWindows, Schedule, SchedulePreview } from '@decryp7/paddock-ui/schedule';

/**
 * The browser demo's stand-in for `paddock_jobs::schedule` (the browser build has no Rust): a fixed "now" and just enough
 * arithmetic to make the editors respond. It is never the reference for behaviour; the real rules (zones, daylight saving,
 * cron, catch-up, validation) are the engine's, tested in `paddock-jobs/tests/schedule.rs`.
 */
export const DEMO_NOW = Date.UTC(2026, 9, 22, 8, 0, 0);

const HOUR = 3_600_000;

export interface MockAnswer {
  readonly preview: SchedulePreview | null;
  readonly problems: readonly string[];
}

/** What the engine's `check` and `preview` would answer, roughly. */
export function mockSchedule(s: Schedule): MockAnswer {
  switch (s.mode) {
    case 'every': {
      const each = Math.max(60, s.secs) * 1000;
      const warnings = s.secs < 60 ? ['This source is checked at most every minute; that is used instead.'] : [];
      return {
        problems: [],
        preview: { description: `Every ${Math.round(s.secs / 60)} minutes`, nextRuns: [1, 2, 3, 4, 5].map((i) => DEMO_NOW + each * i), warnings, zone: 'UTC' },
      };
    }
    case 'times': {
      const problems = [...(s.days.length ? [] : ['Pick at least one day.']), ...(s.times.length ? [] : ['Add at least one time of day.'])];
      if (problems.length) return { preview: null, problems };
      const times = [...s.times].sort();
      const runs: number[] = [];
      for (let day = 0; runs.length < 5 && day < 14; day++) {
        const date = new Date(DEMO_NOW + day * 24 * HOUR);
        const name = (['sun', 'mon', 'tue', 'wed', 'thu', 'fri', 'sat'] as const)[date.getUTCDay()]!;
        if (!s.days.includes(name)) continue;
        for (const t of times) {
          const [h, m] = t.split(':').map(Number);
          const at = Date.UTC(date.getUTCFullYear(), date.getUTCMonth(), date.getUTCDate(), h, m);
          if (at > DEMO_NOW && runs.length < 5) runs.push(at);
        }
      }
      return { problems: [], preview: { description: `${s.days.length} days at ${times.join(', ')}`, nextRuns: runs, warnings: [], zone: s.zone ?? 'UTC' } };
    }
    case 'cron': {
      const fields = s.expr.trim().split(/\s+/).filter(Boolean).length;
      if (fields < 5 || fields > 6) {
        return { preview: null, problems: [`A cron expression has five fields (minute hour day-of-month month day-of-week), but this has ${fields}. Example: 0 9 * * 1-5.`] };
      }
      return { problems: [], preview: { description: `Cron ${s.expr.trim()}`, nextRuns: [1, 2, 3, 4, 5].map((i) => DEMO_NOW + i * 24 * HOUR), warnings: [], zone: s.zone ?? 'UTC' } };
    }
  }
}

/** What `PauseWindows::check` would say, roughly. */
export function mockPause(p: PauseWindows): readonly string[] {
  return p.windows.some((w) => w.start === w.end) ? ['A window needs a start that differs from its end.'] : [];
}
