import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { Router, RouterLink } from '@angular/router';
import { SuEmptyState } from '@decryp7/paddock-ui';
import type { WatchSummary } from '../../core/api.types';
import { kindIcon, STATUS_FILTERS } from '../../shared/labels';
import { AgoPipe, AgoShortPipe, UntilShortPipe } from '../../shared/ui/ago.pipe';
import { Avatar } from '../../shared/ui/avatar';
import { Panel } from '../../shared/ui/panel';
import { MenuButton, type MenuEntry } from '../../shared/ui/row-menu';
import { StatusDot } from '../../shared/ui/status-dot';
import { Version } from '../../shared/ui/version';
import type { Loadable } from '../../state/loadable';
import { WatchesStore, type WatchFilter, type WatchSort } from '../../state/watches.store';

/** The list of every watch: search, filter, sortable columns and the quick actions (on hover and in a menu). */
@Component({
  selector: 'sp-watches-page',
  imports: [MatIcon, RouterLink, SuEmptyState, Panel, Avatar, StatusDot, Version, MenuButton, AgoPipe, AgoShortPipe, UntilShortPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './watches-page.html',
  styleUrl: './watches-page.scss',
})
export class WatchesPage {
  protected readonly store = inject(WatchesStore);
  private readonly router = inject(Router);

  protected readonly listState = computed<Loadable<unknown>>(() => ({
    // A first load that failed has nothing to show: the panel shows the error with Retry, not an empty list.
    value: this.store.loaded() && !(this.store.error() && !this.store.watches().length) ? this.store.watches() : null,
    loading: this.store.loading(),
    error: this.store.error(),
  }));

  protected readonly filters = STATUS_FILTERS;
  protected readonly glyph = kindIcon;

  protected readonly headEntries = computed<MenuEntry[]>(() => [
    { id: 'check', label: 'Check all now', icon: 'refresh', disabled: this.store.checking() || !this.store.watches().length },
    { id: 'seen', label: 'Mark all seen', icon: 'check-all', disabled: !this.store.unseen().length },
  ]);

  protected onHead(id: string): void {
    if (id === 'check') void this.store.checkNow(null);
    else if (id === 'seen') void this.store.acknowledgeAll();
  }

  protected rowEntries(w: WatchSummary): MenuEntry[] {
    return [
      { id: 'open', label: 'Open details', icon: 'watches' },
      ...(w.unseen ? [{ id: 'seen', label: 'Mark as seen', icon: 'check' }] : []),
      { id: 'pause', label: w.paused ? 'Resume' : 'Pause', icon: w.paused ? 'play' : 'pause' },
    ];
  }

  protected onRow(id: string, w: WatchSummary): void {
    if (id === 'open') this.open(w.id);
    else if (id === 'seen') void this.store.acknowledge(w.id, null);
    else if (id === 'pause') void this.store.setPaused(w.id, !w.paused);
  }

  protected open(id: string): void {
    void this.router.navigate(['/watches', id]);
  }

  protected ariaSort(by: WatchSort): string | null {
    return this.store.sort() === by ? 'ascending' : null;
  }

  protected onQuery(e: Event): void {
    this.store.setQuery((e.target as HTMLInputElement).value);
  }

  protected setFilter(v: string): void {
    this.store.setFilter(v as WatchFilter);
  }

  protected clearFilters(): void {
    this.store.setQuery('');
    this.store.setFilter('all');
  }
}
