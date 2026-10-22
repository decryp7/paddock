import type { TuningCategory, TuningSuggestion } from '../../core/api.types';

/** The groups of the page, in the order they are shown, with the sentence under each title. */
export const CATEGORIES: { id: TuningCategory; title: string; text: string }[] = [
  { id: 'power', title: 'Power', text: 'The power plan or mode while you are plugged in.' },
  { id: 'sleep', title: 'Sleep and hibernation', text: 'When the computer sleeps, and the space hibernation keeps.' },
  { id: 'battery', title: 'Battery', text: 'Care that makes the battery last for years.' },
  { id: 'developer', title: 'Developer folders', text: 'Search indexing and antivirus scanning of code you build.' },
  { id: 'appearance', title: 'Appearance and background', text: 'Animations, transparency and apps that run unseen.' },
];

export interface TuningGroup {
  id: TuningCategory;
  title: string;
  text: string;
  items: TuningSuggestion[];
}

/** The suggestions grouped for the page: known groups in order, empty ones left out, applied changes first within a group. */
export function groupTuning(items: TuningSuggestion[]): TuningGroup[] {
  return CATEGORIES.map((c) => ({ ...c, items: items.filter((t) => t.category === c.id).sort((a, b) => Number(b.applied) - Number(a.applied)) })).filter((g) => g.items.length > 0);
}

/** What the action button of a suggestion does. */
export type TuningAction = 'review' | 'revert' | 'revertAnyway' | 'blocked';

export function actionFor(t: TuningSuggestion): TuningAction {
  if (t.applied) return t.changedOutside ? 'revertAnyway' : 'revert';
  return t.blocked ? 'blocked' : 'review';
}

/** The counts shown under the title: applied by Dyno, and ready to apply. */
export function summary(items: TuningSuggestion[]): string {
  const applied = items.filter((t) => t.applied).length;
  const ready = items.filter((t) => !t.applied && !t.blocked).length;
  if (items.length === 0) return 'Nothing to tune on this computer right now.';
  const parts = [`${ready} ready to review`];
  if (applied > 0) parts.push(`${applied} applied by Dyno`);
  const waiting = items.length - applied - ready;
  if (waiting > 0) parts.push(`${waiting} waiting for administrator rights`);
  return parts.join(' · ');
}
