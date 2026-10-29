import { ChangeDetectionStrategy, Component, computed, DestroyRef, effect, inject, input, signal, untracked } from '@angular/core';
import { DumpMarker } from '../core/api.types';
import { formatDateTime } from '../core/format';
import { WorkspaceStore } from '../core/workspace.store';
import { COMMAND_CONTEXT } from '../extensions/command-registry';
import { headlineText } from './dump-headline';
import { DumpsBackend } from './dumps-backend';
import { DumpsStore } from './dumps.store';
import { StripChip, TimelineChips } from '../timeline/timeline-chips';

/** A marker placed on the visible log range (percent of the plot). */
export interface MarkerVm {
  marker: DumpMarker;
  left: number;
}

/** Estimated label width in px (as the profile span labels). */
const labelWidth = (name: string) => Math.min(200, 14 + name.length * 6.5);

/** Places markers inside the visible range (dropping the others). Pure. */
export function layoutMarkers(markers: readonly DumpMarker[], view: { from: number; to: number }): MarkerVm[] {
  const span = Math.max(1, view.to - view.from);
  return markers
    .filter((m) => m.timeMs >= view.from && m.timeMs <= view.to)
    .sort((a, b) => a.timeMs - b.timeMs || a.id - b.id)
    .map((m) => ({ marker: m, left: ((m.timeMs - view.from) / span) * 100 }));
}

/**
 * Dumps and UMDH snapshots as markers on the log timeline, at the time the dump was written (or the snapshot's file
 * time). The label opens the document. Renders nothing without the dumps backend.
 */
@Component({
  selector: 'debrief-dump-marker-track',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (m of items(); track m.marker.id) {
      <div class="line" [class.critical]="m.marker.severity === 'critical'" [style.left.%]="m.left" aria-hidden="true"></div>
    }
  `,
  styles: `
    :host { position: absolute; inset: 0; pointer-events: none; }
    .line { position: absolute; top: var(--tl-strip-top, 50px); bottom: 0; width: 0; border-left: 2px dotted var(--s3); }
    .line.critical { border-left-color: var(--lvl-fatal); }
  `,
})
export class DumpMarkerTrack {
  /** The visible log range (Unix ms). */
  readonly view = input<{ from: number; to: number } | null>(null);
  /** Plot width in px (for stacking labels). */
  readonly width = input(1000);
  private readonly backend = inject(DumpsBackend, { optional: true });
  /** Only with the dumps feature installed (the store needs its backend). */
  private readonly store = this.backend ? inject(DumpsStore) : null;
  private readonly context = inject(COMMAND_CONTEXT, { optional: true });
  private readonly workspace = inject(WorkspaceStore);
  private readonly chips = inject(TimelineChips, { optional: true });
  private readonly markers = signal<DumpMarker[]>([]);

  protected readonly items = computed(() => {
    const v = this.view();
    return v ? layoutMarkers(this.markers(), v) : [];
  });

  constructor() {
    effect(() => {
      this.store?.version();
      untracked(() => void this.load());
    });
    const chips = this.chips;
    if (chips) {
      chips.register('dump', computed(() => this.items().map((m) => this.chip(m))));
      inject(DestroyRef).onDestroy(() => chips.clear('dump'));
    }
  }

  private chip(m: MarkerVm): StripChip {
    const text = `${m.marker.kind === 'dump' ? '✖ ' : '▲ '}${m.marker.name}`;
    return {
      id: `dump-${m.marker.id}`,
      x: (m.left / 100) * this.width(),
      w: labelWidth(m.marker.name),
      text,
      title: this.title(m),
      label: this.label(m),
      testid: 'dump-marker',
      accent: 'var(--border-3)',
      open: () => this.open(m),
    };
  }

  private async load(): Promise<void> {
    if (!this.backend) return;
    try {
      this.markers.set(await this.backend.markers());
    } catch {
      // The timeline keeps working without markers; the backend wrapper logs the failure.
      this.markers.set([]);
    }
  }

  protected title(m: MarkerVm): string {
    const when = formatDateTime(m.marker.timeMs, this.workspace.timezone(), true);
    const name = m.marker.name;
    const headline = headlineText(m.marker.headline);
    return $localize`:@@dumps.marker.title:${name}:name: — ${when}:when:: ${headline}:headline:. Click to open.`;
  }

  protected label(m: MarkerVm): string {
    const name = m.marker.name;
    return m.marker.kind === 'dump' ? $localize`:@@dumps.marker.openDump:Open dump ${name}:name:` : $localize`:@@dumps.marker.openUmdh:Open UMDH log ${name}:name:`;
  }

  protected open(m: MarkerVm): void {
    if (m.marker.kind === 'dump') {
      this.store?.setActiveDump(m.marker.id);
      this.context?.showView('dump');
    } else {
      this.store?.setUmdh(m.marker.id, this.store.umdhBase());
      this.context?.showView('umdh');
    }
  }
}
