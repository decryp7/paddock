import { inject, Injector } from '@angular/core';
import type { RecordMenuContribution } from '../../context/record-actions';

/** "Add to report" in the record context menu: the record, or every record of the grid's multi-selection. */
export function reportMenuItems(): RecordMenuContribution {
  // The report's code loads when the entry is used, not with the main window (debrief-1-2-api.md §14.3).
  const injector = inject(Injector);
  const addEvents = (ids: number[]) => import('./report-actions').then((m) => injector.get(m.ReportActions).addEvents(ids));
  return (record, target) => {
    const ids = target.records?.length ? target.records.map((r) => r.id) : [record.id];
    return [{ label: ids.length > 1 ? $localize`:@@share.reportMenu.addMany:Add ${ids.length}:count: events to report` : $localize`:@@share.reportMenu.add:Add to report`, run: () => void addEvents(ids) }];
  };
}
