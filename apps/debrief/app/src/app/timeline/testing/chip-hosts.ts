import { Component, input } from '@angular/core';
import { DumpMarkerTrack } from '../../dumps/dump-marker-track';
import { NoteTimelineTrack } from '../../notes/note-timeline-track';
import { ProfileSpanTrack } from '../../profiling/integration/profile-span-track';
import { TimelineChipStrip } from '../timeline-chip-strip';
import { TimelineChips } from '../timeline-chips';

/** The inputs every host takes, as the tracks do. */
type Range = { from: number; to: number } | null;

/** A track and the chip strip it feeds, as in the timeline panel, with room for every chip (no overflow menu). */
@Component({
  imports: [ProfileSpanTrack, TimelineChipStrip],
  providers: [TimelineChips],
  template: `<debrief-timeline-chip-strip [width]="width()" [rows]="99" /><debrief-profile-span-track [view]="view()" [width]="width()" />`,
})
export class ProfileSpanHost {
  readonly view = input<Range>(null);
  readonly width = input(1000);
}

@Component({
  imports: [DumpMarkerTrack, TimelineChipStrip],
  providers: [TimelineChips],
  template: `<debrief-timeline-chip-strip [width]="width()" [rows]="99" /><debrief-dump-marker-track [view]="view()" [width]="width()" />`,
})
export class DumpMarkerHost {
  readonly view = input<Range>(null);
  readonly width = input(1000);
}

@Component({
  imports: [NoteTimelineTrack, TimelineChipStrip],
  providers: [TimelineChips],
  template: `<debrief-timeline-chip-strip [width]="width()" [rows]="99" /><debrief-note-timeline-track [view]="view()" [width]="width()" />`,
})
export class NoteHost {
  readonly view = input<Range>(null);
  readonly width = input(1000);
}
