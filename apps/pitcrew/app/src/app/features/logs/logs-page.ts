import { CdkFixedSizeVirtualScroll, CdkVirtualForOf, CdkVirtualScrollViewport } from '@angular/cdk/scrolling';
import { ChangeDetectionStrategy, Component, DestroyRef, computed, effect, inject, input, untracked, viewChild } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { MatTooltip } from '@angular/material/tooltip';
import { Router } from '@angular/router';
import type { LogLine } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { ClockService } from '../../core/clock.service';
import { EmptyState } from '../../shared/ui/empty-state';
import { StatusStore } from '../../state/status.store';
import { ToastStore } from '../../state/toast.store';
import { highlight, levelTone, parseLogLine } from './log-line';
import { LogsStore } from './logs.store';

/** How often to fetch new lines while following (and the window is visible). */
export const FOLLOW_MS = 2_000;
const ROW = 22;

/** A runner's log: virtual scrolling, follow mode, plain-text search and copying a line. */
@Component({
  selector: 'pc-logs-page',
  imports: [CdkVirtualScrollViewport, CdkFixedSizeVirtualScroll, CdkVirtualForOf, MatIcon, MatTooltip, EmptyState],
  providers: [LogsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './logs-page.html',
  styleUrl: './logs-page.scss',
})
export class LogsPage {
  /** Query parameter. */
  readonly runner = input<string | undefined>(undefined);

  protected readonly status = inject(StatusStore);
  protected readonly store = inject(LogsStore);
  private readonly clock = inject(ClockService);
  private readonly router = inject(Router);
  private readonly backend = inject(BACKEND);
  private readonly toasts = inject(ToastStore);
  private readonly viewport = viewChild(CdkVirtualScrollViewport);

  protected readonly row = ROW;
  protected readonly names = computed(() => this.status.runners().map((r) => r.name));
  protected readonly selected = computed(() => {
    const r = this.runner();
    return r && this.names().includes(r) ? r : (this.names()[0] ?? null);
  });
  protected readonly shown = computed(() => {
    const lines = this.store.results() ?? this.store.lines();
    const q = this.store.results() ? this.store.query() : '';
    return lines.map((l) => {
      const p = parseLogLine(l.text);
      return { seq: l.seq, text: l.text, time: p.time, level: p.level, tone: levelTone(p.level), parts: highlight(p.msg, q) };
    });
  });

  private searchTimer: ReturnType<typeof setTimeout> | null = null;
  private lastOffset = 0;

  constructor() {
    effect(() => {
      const r = this.selected();
      if (r) untracked(() => void this.store.open(r).then(() => this.toBottom()));
    });
    const timer = setInterval(() => {
      if (this.store.following() && this.clock.visible() && this.store.results() === null) {
        void this.store.poll().then((n) => {
          if (n > 0) this.toBottom();
        });
      }
    }, FOLLOW_MS);
    inject(DestroyRef).onDestroy(() => {
      clearInterval(timer);
      if (this.searchTimer) clearTimeout(this.searchTimer);
    });
  }

  protected pick(name: string): void {
    void this.router.navigate([], { queryParams: { runner: name }, replaceUrl: true });
  }

  protected onQuery(value: string): void {
    if (this.searchTimer) clearTimeout(this.searchTimer);
    this.searchTimer = setTimeout(() => void this.store.search(value), 250);
  }

  protected clear(input: HTMLInputElement): void {
    input.value = '';
    this.store.clearSearch();
    this.toBottom();
  }

  protected toggleFollow(): void {
    const on = !this.store.following();
    this.store.setFollowing(on);
    if (on) this.toBottom();
  }

  /** Scrolling up stops following; scrolling to the end resumes it. */
  protected onScroll(): void {
    const v = this.viewport();
    if (!v || this.store.results()) return;
    const offset = v.measureScrollOffset('top');
    const atEnd = v.measureScrollOffset('bottom') < ROW * 2;
    if (atEnd && !this.store.following()) this.store.setFollowing(true);
    else if (!atEnd && offset < this.lastOffset && this.store.following()) this.store.setFollowing(false);
    this.lastOffset = offset;
  }

  protected async copy(line: LogLine): Promise<void> {
    try {
      await navigator.clipboard.writeText(line.text);
      this.toasts.show({ tone: 'success', icon: 'copy', title: 'Line copied', body: null });
    } catch {
      this.toasts.show({ tone: 'warning', icon: 'alert', title: 'Could not copy', body: 'The clipboard is not available here.' });
    }
  }

  protected reveal(): void {
    const r = this.selected();
    if (r) void this.backend.revealLogs(r).catch(() => undefined);
  }

  protected trackSeq(_: number, l: { seq: number }): number {
    return l.seq;
  }

  private toBottom(): void {
    // After the viewport has rendered the new lines.
    setTimeout(() => {
      const v = this.viewport();
      if (!v || !this.store.following()) return;
      v.checkViewportSize();
      v.scrollTo({ bottom: 0 });
      this.lastOffset = v.measureScrollOffset('top');
    }, 0);
  }
}
