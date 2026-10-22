import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';
import { MatIcon } from '@angular/material/icon';
import { RouterLink } from '@angular/router';
import { SuEmptyState, SuSegmentedControl, type SuSegment } from '@decryp7/paddock-ui';
import { AgoPipe } from '../../shared/ui/ago.pipe';
import { KindIcon } from '../../shared/ui/kind-icon';
import type { Loadable } from '../../state/loadable';
import { Panel } from '../../shared/ui/panel';
import { WatchStatus } from '../../shared/ui/watch-status';
import { WatchesStore, type WatchFilter, type WatchSort } from '../../state/watches.store';

/** The list of every watch: search, filter, sort and the quick actions. */
@Component({
  selector: 'sp-watches-page',
  imports: [MatIcon, RouterLink, SuEmptyState, SuSegmentedControl, Panel, KindIcon, WatchStatus, AgoPipe],
  changeDetection: ChangeDetectionStrategy.OnPush,
  templateUrl: './watches-page.html',
  styleUrl: './watches-page.scss',
})
export class WatchesPage {
  protected readonly store = inject(WatchesStore);

  protected readonly listState = computed<Loadable<unknown>>(() => ({
    // A first load that failed has nothing to show: the panel shows the error with Retry, not an empty list.
    value: this.store.loaded() && !(this.store.error() && !this.store.watches().length) ? this.store.watches() : null,
    loading: this.store.loading(),
    error: this.store.error(),
  }));

  protected readonly filters: SuSegment[] = [
    { value: 'all', label: 'All' },
    { value: 'new', label: 'New' },
    { value: 'attention', label: 'Needs attention' },
    { value: 'paused', label: 'Paused' },
  ];

  protected onQuery(e: Event): void {
    this.store.setQuery((e.target as HTMLInputElement).value);
  }

  protected onSort(e: Event): void {
    this.store.setSort((e.target as HTMLSelectElement).value as WatchSort);
  }

  protected setFilter(v: string): void {
    this.store.setFilter(v as WatchFilter);
  }

  protected clearFilters(): void {
    this.store.setQuery('');
    this.store.setFilter('all');
  }
}
