import type { DueFile, SweepPlan, SweepSummary, WatchedFolderView } from '../../core/api.types';
import { formatBytes, formatRelative, plural } from '../../shared/format';

/** The keep-for times offered (the engine accepts 10 minutes to 10 days; these are the usual choices). */
export const KEEP_PRESETS: readonly { secs: number; label: string }[] = [
  { secs: 600, label: '10 minutes' },
  { secs: 3600, label: '1 hour' },
  { secs: 6 * 3600, label: '6 hours' },
  { secs: 86_400, label: '1 day' },
  { secs: 2 * 86_400, label: '2 days' },
  { secs: 3 * 86_400, label: '3 days' },
  { secs: 7 * 86_400, label: '7 days' },
  { secs: 10 * 86_400, label: '10 days' },
];

/** A keep-for time in words ("10 minutes", "1 hour", "2 days"), as the engine words it. */
export function keepLabel(secs: number): string {
  if (secs >= 86_400 && secs % 86_400 === 0) return plural(secs / 86_400, 'day');
  if (secs >= 3600 && secs % 3600 === 0) return plural(secs / 3600, 'hour');
  return plural(Math.ceil(secs / 60), 'minute');
}

/** The presets, plus the current value when it is not one of them (an older choice stays selectable). */
export function keepOptions(current: number): { secs: number; label: string }[] {
  const list = [...KEEP_PRESETS];
  if (!list.some((p) => p.secs === current)) list.push({ secs: current, label: keepLabel(current) });
  return list.sort((a, b) => a.secs - b.secs);
}

/** The folder's own name from its path (either separator). */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]+/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** Keeps the start and the end of a long path: `/Users/alex/…/build`. */
export function shortPath(path: string, max = 46): string {
  if (path.length <= max) return path;
  const keep = max - 1;
  const head = Math.ceil(keep * 0.45);
  const tail = keep - head;
  return `${path.slice(0, head)}…${path.slice(path.length - tail)}`;
}

export type Tone = 'ok' | 'warn' | null;

/** The state of a folder as a dot and a word: calm when it is simply on or off, a warning only when something is wrong. */
export function statusOf(v: WatchedFolderView): { tone: Tone; text: string } {
  if (!v.exists) return { tone: 'warn', text: 'Folder missing' };
  if (v.folder.lastSweep && v.folder.lastSweep.failed > 0 && v.folder.lastSweep.moved === 0 && v.folder.lastSweep.firstFailure) {
    return { tone: 'warn', text: 'Could not sweep' };
  }
  return v.folder.enabled ? { tone: 'ok', text: 'On' } : { tone: null, text: v.folder.previewed ? 'Off' : 'Off, not previewed' };
}

/** What the last sweep did, in a few words. */
export function sweepText(s: SweepSummary | null, now: number): string {
  if (!s) return 'Not yet';
  const when = formatRelative(s.at, now);
  if (s.failed > 0 && s.moved === 0) return `${when}: ${s.firstFailure ?? 'could not sweep'}`;
  if (s.moved === 0) return `${when}: nothing due`;
  const stayed = s.failed > 0 ? `, ${s.failed} stayed` : '';
  return `${when}: moved ${plural(s.moved, 'file')} (${formatBytes(s.bytes)})${stayed}`;
}

/** The sentence at the top of the preview. */
export function dueSummary(plan: SweepPlan, keepSecs: number): string {
  if (plan.dueTotal === 0) return `Nothing is older than ${keepLabel(keepSecs)} yet.`;
  return `${plural(plan.dueTotal, 'file')} (${formatBytes(plan.dueBytes)}) ${plan.dueTotal === 1 ? 'is' : 'are'} older than ${keepLabel(keepSecs)} and would move to the 7-day quarantine.`;
}

/** The small print under the preview: what stays and why. */
export function staysText(plan: SweepPlan): string {
  const parts: string[] = [];
  if (plan.waiting > 0) parts.push(`${plural(plan.waiting, 'matching file')} not old enough yet`);
  if (plan.other > 0) parts.push(`${plural(plan.other, 'file')} left alone (they do not match)`);
  if (plan.links > 0) parts.push(`${plural(plan.links, 'link')} skipped (links are never followed)`);
  if (plan.refused > 0) parts.push(`${plural(plan.refused, 'file')} refused by the safety rules`);
  if (plan.dueTotal > plan.due.length) parts.push(`the list shows the oldest ${plan.due.length} of ${plan.dueTotal}`);
  if (plan.truncated) parts.push('the folder is very large, so only part of it was looked at');
  return parts.length === 0 ? '' : `${parts.join('; ')}.`;
}

/** A due file as a table row: its name, size and how long it has been there. */
export function dueRow(f: DueFile, now: number): { name: string; path: string; size: string; age: string } {
  return { name: folderName(f.path), path: f.path, size: formatBytes(f.bytes), age: formatRelative(f.arrived, now).replace(' ago', '') };
}
