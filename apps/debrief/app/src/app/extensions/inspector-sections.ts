import { Injectable } from '@angular/core';
import { RecordView } from '../core/api.types';
import { contributedItems, DEBRIEF_KIND } from './contributions';
import { DebriefInspectorSection } from './tokens';

/** Plugin sections for the inspector, ordered, filtered per record. */
@Injectable({ providedIn: 'root' })
export class InspectorSections {
  private readonly sections = contributedItems<DebriefInspectorSection>(DEBRIEF_KIND.inspectorSections).sort(
    (a, b) => a.order - b.order,
  );

  forRecord(record: RecordView): DebriefInspectorSection[] {
    return this.sections.filter((s) => {
      try {
        return s.when ? s.when(record) : true;
      } catch {
        return false;
      }
    });
  }
}
