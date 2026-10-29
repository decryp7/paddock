import { ChangeDetectionStrategy, Component, computed, DestroyRef, inject, input } from '@angular/core';
import { formatDateTime } from '../core/format';
import { WorkspaceStore } from '../core/workspace.store';
import { StripChip, TimelineChips } from '../timeline/timeline-chips';
import { NoteActions } from './note-actions';
import { severityColor, TimeNote, timeNotes } from './notes-model';
import { NotesStore } from './notes.store';

/** A note placed on the visible range: its position and width (percent of the plot) and label row. */
export interface NoteMarkVm {
  note: TimeNote;
  left: number;
  /** Width of a range (percent); 0 for a moment. */
  width: number;
}

/** Places notes of the visible range (percent of the plot). Pure. */
export function layoutNoteMarks(notes: readonly TimeNote[], view: { from: number; to: number }): NoteMarkVm[] {
  const span = Math.max(1, view.to - view.from);
  return [...notes]
    .sort((a, b) => a.from - b.from || a.id.localeCompare(b.id))
    .map((note) => {
      const from = Math.max(note.from, view.from);
      const to = Math.min(note.to, view.to);
      return { note, left: ((from - view.from) / span) * 100, width: note.to > note.from ? ((to - from) / span) * 100 : 0 };
    });
}

/**
 * Notes on moments, ranges, markers and chart points as marks on the timeline (ADR 0021 §5): a flag with the note's
 * first line; a range also shades its span. Clicking a flag edits the note.
 */
@Component({
  selector: 'debrief-note-timeline-track',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (m of items(); track m.note.id) {
      @if (m.width > 0) {
        <div class="span" [style.left.%]="m.left" [style.width.%]="m.width" [style.--c]="color(m)" aria-hidden="true"></div>
      } @else {
        <div class="line" [style.left.%]="m.left" [style.--c]="color(m)" aria-hidden="true"></div>
      }
    }
  `,
  styles: `
    :host { position: absolute; inset: 0; pointer-events: none; }
    .line { position: absolute; top: 0; bottom: 0; width: 0; border-left: 2px dashed var(--c); opacity: .8; }
    .span { position: absolute; top: 0; bottom: 0; background: color-mix(in srgb, var(--c) 10%, transparent); border-left: 1px dashed var(--c); border-right: 1px dashed var(--c); }
  `,
})
export class NoteTimelineTrack {
  /** The visible range (Unix ms). */
  readonly view = input<{ from: number; to: number } | null>(null);
  /** Plot width in px (for stacking labels). */
  readonly width = input(1000);
  private readonly store = inject(NotesStore);
  private readonly actions = inject(NoteActions);
  private readonly workspace = inject(WorkspaceStore);
  private readonly chips = inject(TimelineChips, { optional: true });

  protected readonly items = computed(() => {
    const v = this.view();
    return v ? layoutNoteMarks(timeNotes(this.store.items(), v.from, v.to), v) : [];
  });

  constructor() {
    const chips = this.chips;
    if (!chips) return;
    chips.register('note', computed(() => this.items().map((m) => this.chip(m))));
    inject(DestroyRef).onDestroy(() => chips.clear('note'));
  }

  private chip(m: NoteMarkVm): StripChip {
    return {
      id: `note-${m.note.id}`,
      x: (m.left / 100) * this.width(),
      w: Math.min(180, 20 + m.note.label.length * 6),
      text: `✎ ${m.note.label}`,
      title: this.title(m),
      label: this.title(m),
      testid: 'note-mark',
      accent: this.color(m),
      open: () => this.open(m),
    };
  }

  protected color(m: NoteMarkVm): string {
    return severityColor(m.note.severity);
  }

  protected title(m: NoteMarkVm): string {
    const when = formatDateTime(m.note.from, this.workspace.timezone(), true);
    const label = m.note.label;
    return $localize`:@@notes.timeline.title:Note at ${when}:when:: ${label}:label:. Click to edit.`;
  }

  protected open(m: NoteMarkVm): void {
    const n = this.store.get(m.note.id);
    if (n) void this.actions.edit(n);
  }
}
