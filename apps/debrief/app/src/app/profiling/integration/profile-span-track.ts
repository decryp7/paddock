import { ChangeDetectionStrategy, Component, computed, DestroyRef, effect, inject, input, signal, untracked } from '@angular/core';
import { ProfileSpan, ProfileTimeRange } from '../../core/api.types';
import { formatDateTime, formatDuration } from '../../core/format';
import { WorkspaceStore } from '../../core/workspace.store';
import { StripChip, TimelineChips } from '../../timeline/timeline-chips';
import { ProfilingBackend } from '../profiling-backend';
import { ProfileViewState } from '../profile-view-state.store';
import { ProfilingNavigator } from '../profiling-navigator';

/** A capture span placed on the visible log range (percent of the plot). */
export interface SpanVm {
  span: ProfileSpan;
  left: number;
  width: number;
  /** The part of the capture inside the visible range, in ns since its start (null = all of it). */
  range: ProfileTimeRange | null;
}

/** Estimated label width in px (monospace-ish 10px font, capped like the CSS). */
export const labelWidth = (name: string) => Math.min(220, 12 + name.length * 6.5);

/** Places spans on the visible range (dropping those outside). Pure. */
export function layoutSpans(spans: readonly ProfileSpan[], view: { from: number; to: number }): SpanVm[] {
  const span = Math.max(1, view.to - view.from);
  return spans
    .filter((s) => s.toMs >= view.from && s.fromMs <= view.to)
    .sort((a, b) => a.fromMs - b.fromMs || a.id - b.id)
    .map((s) => {
      const from = Math.max(s.fromMs, view.from);
      const to = Math.min(s.toMs, view.to);
      const clipped = s.capture === 'profile' && (from > s.fromMs || to < s.toMs);
      const left = ((from - view.from) / span) * 100;
      return {
        span: s,
        left,
        width: ((to - from) / span) * 100,
        range: clipped ? { fromNs: Math.round((from - s.fromMs) * 1e6), toNs: Math.round((to - s.fromMs) * 1e6) } : null,
      };
    });
}

/**
 * Profile capture spans as labelled bands on the log timeline (heap snapshots as a line). The label
 * opens the profile on the visible part of the capture. Renders nothing without the profiling plugin.
 */
@Component({
  selector: 'debrief-profile-span-track',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @for (s of items(); track s.span.id) {
      <div class="band" [class.heap]="s.span.capture === 'heap'" [style.left.%]="s.left" [style.width.%]="s.width" aria-hidden="true"></div>
    }
  `,
  styles: `
    :host { position: absolute; inset: 0; pointer-events: none; }
    .band { position: absolute; top: calc(var(--tl-strip-top, 50px) + var(--tl-strip-h, 0px)); bottom: 0; min-width: 2px; background: color-mix(in srgb, var(--s4) 12%, transparent); border-left: 1px dashed var(--s4); border-right: 1px dashed var(--s4); }
    .band.heap { background: none; border-right: 0; }
  `,
})
export class ProfileSpanTrack {
  /** The visible log range (Unix ms). */
  readonly view = input<{ from: number; to: number } | null>(null);
  /** Plot width in px (for stacking labels). */
  readonly width = input(1000);
  private readonly backend = inject(ProfilingBackend, { optional: true });
  private readonly state = inject(ProfileViewState);
  private readonly nav = inject(ProfilingNavigator);
  private readonly workspace = inject(WorkspaceStore);
  private readonly chips = inject(TimelineChips, { optional: true });
  private readonly spans = signal<ProfileSpan[]>([]);

  protected readonly items = computed(() => {
    const v = this.view();
    return v ? layoutSpans(this.spans(), v) : [];
  });

  constructor() {
    effect(() => {
      this.state.capturesVersion();
      untracked(() => void this.load());
    });
    const chips = this.chips;
    if (chips) {
      chips.register('profile', computed(() => this.items().map((s) => this.chip(s))));
      inject(DestroyRef).onDestroy(() => chips.clear('profile'));
    }
  }

  private chip(s: SpanVm): StripChip {
    return {
      id: `profile-${s.span.id}`,
      x: (s.left / 100) * this.width(),
      w: labelWidth(s.span.name),
      text: `${s.span.capture === 'heap' ? '◆ ' : '▶ '}${s.span.name}`,
      title: this.title(s),
      label: this.openLabel(s),
      testid: 'profile-span',
      accent: 'var(--s4)',
      open: () => this.open(s),
    };
  }

  private async load(): Promise<void> {
    if (!this.backend) return;
    try {
      this.spans.set(await this.backend.spans());
    } catch {
      // The timeline keeps working without spans; the backend wrapper logs the failure.
      this.spans.set([]);
    }
  }

  protected title(s: SpanVm): string {
    const tz = this.workspace.timezone();
    const when = s.span.fromMs === s.span.toMs ? formatDateTime(s.span.fromMs, tz, true) : `${formatDateTime(s.span.fromMs, tz, true)} (${formatDuration(s.span.toMs - s.span.fromMs)})`;
    const name = s.span.name;
    return $localize`:@@profiling.spanTrack.title:${name}:name: — ${when}:when:. Click to open.`;
  }

  protected openLabel(s: SpanVm): string {
    const name = s.span.name;
    if (s.span.capture === 'heap') {
      return s.range
        ? $localize`:@@profiling.spanTrack.openHeapRange:Open heap snapshot ${name}:name: on the visible range`
        : $localize`:@@profiling.spanTrack.openHeap:Open heap snapshot ${name}:name:`;
    }
    return s.range
      ? $localize`:@@profiling.spanTrack.openProfileRange:Open capture ${name}:name: on the visible range`
      : $localize`:@@profiling.spanTrack.openProfile:Open capture ${name}:name:`;
  }

  protected open(s: SpanVm): void {
    if (s.span.capture === 'heap') this.nav.openHeap(s.span.id);
    else this.nav.openOnRange(s.span.id, s.range);
  }
}
