import type { HistoryEvent, Timestamp } from '../../../core/api.types';

/** What the runner declared to the server after its last restart, compared with the file (from the history). */
export type LabelCheck =
  | { kind: 'verified'; at: Timestamp; labels: string[] }
  | { kind: 'mismatch'; at: Timestamp; expected: string[]; declared: string[] };

/** The newest label check among the events (newest first, as the activity reply has them). */
export function latestLabelCheck(events: readonly HistoryEvent[]): LabelCheck | null {
  for (const e of events) {
    if (e.kind === 'labelsVerified') return { kind: 'verified', at: e.at, labels: e.labels };
    if (e.kind === 'labelsMismatch') return { kind: 'mismatch', at: e.at, expected: e.expected, declared: e.declared };
  }
  return null;
}
