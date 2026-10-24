import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { SuEmptyState, SuErrorState, SuSkeleton } from '@decryp7/paddock-ui';
import { formatBytes, formatDateTime } from '@decryp7/paddock-ui/format';
import type { ActivityEntry } from './activity-wire';
import { ActivityStore } from './activity.store';

/**
 * The Activity page (plan X44): what the app did on the user's behalf, newest first, each with its time, what and why, a
 * size where there is one, and Undo while the engine says it is still possible ("Undone" and "No longer possible"
 * otherwise). A table (rows scan best), an inline error with Retry, an empty state; narrow page areas drop the kind and the
 * size. Presentational over `ActivityStore`.
 */
@Component({
  selector: 'pd-activity-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuEmptyState, SuErrorState, SuSkeleton],
  template: `
    <header class="pd-head">
      <h1>{{ heading }}</h1>
      <p class="pd-sub">{{ subtitle }}</p>
    </header>
    @if (store.requestError(); as cause) {
      <pd-error-state [heading]="readFailed" [cause]="cause" [busy]="store.isLoading()" (retry)="store.load()" />
    }
    @if (store.entries().length) {
      <table data-testid="activity-table">
        <caption class="pd-sr">{{ heading }}</caption>
        <thead>
          <tr>
            <th scope="col" class="pd-when">{{ whenText }}</th>
            <th scope="col">{{ whatText }}</th>
            <th scope="col" class="pd-size">{{ sizeText }}</th>
            <th scope="col" class="pd-undo">{{ undoText }}</th>
          </tr>
        </thead>
        <tbody>
          @for (e of store.entries(); track e.id) {
            <tr [attr.data-testid]="'activity-' + e.id">
              <td class="pd-when">{{ when(e) }}</td>
              <td class="pd-what">
                <span class="pd-title" [attr.title]="e.title">{{ e.title }}</span>
                @if (e.detail) {
                  <span class="pd-detail" [attr.title]="e.detail">{{ e.detail }}</span>
                }
              </td>
              <td class="pd-size">{{ size(e) }}</td>
              <td class="pd-undo">
                @switch (e.undo) {
                  @case ('available') {
                    <button type="button" class="pd-btn pd-btn--sm" [disabled]="store.undoing().includes(e.id)" [attr.aria-label]="undoLabel(e)" (click)="store.undo(e.id)">{{ undoButton }}</button>
                  }
                  @case ('undone') {
                    <span class="pd-state">{{ undoneText }}</span>
                  }
                  @case ('expired') {
                    <span class="pd-state">{{ expiredText }}</span>
                  }
                }
              </td>
            </tr>
          }
        </tbody>
      </table>
    } @else if (store.isLoading()) {
      <div aria-busy="true" [attr.aria-label]="heading"><pd-skeleton shape="block" /></div>
    } @else if (!store.requestError()) {
      <pd-empty-state [heading]="emptyHeading" [description]="emptyDescription" />
    }
  `,
  styles: `
    :host { display: flex; flex-direction: column; gap: var(--pd-space-4); width: 100%; max-width: var(--pd-content-max); margin: 0 auto; padding: var(--pd-space-6); box-sizing: border-box; min-width: 0; }
    @container pd-page (max-width: 480px) { :host { padding: var(--pd-space-4); } }
    .pd-head { display: flex; flex-direction: column; gap: var(--pd-space-1); }
    h1 { margin: 0; font-size: var(--pd-text-title); }
    .pd-sub { margin: 0; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    table { width: 100%; border-collapse: collapse; font-size: var(--pd-text-body); table-layout: fixed; }
    th, td { padding: var(--pd-space-1) var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); text-align: start; vertical-align: middle; }
    th { font-size: var(--pd-text-label); font-weight: var(--pd-weight-medium); color: var(--pd-text-muted); }
    tr:hover td { background: var(--pd-hover); }
    td { height: var(--pd-row-height); }
    .pd-when { width: 9.5rem; color: var(--pd-text-muted); white-space: nowrap; }
    .pd-size { width: 6rem; text-align: end; font-variant-numeric: tabular-nums; color: var(--pd-text-muted); }
    .pd-undo { width: 7rem; text-align: end; }
    .pd-what { min-width: 0; }
    .pd-title, .pd-detail { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .pd-detail { font-size: var(--pd-text-label); color: var(--pd-text-muted); }
    .pd-state { color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .pd-sr { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); white-space: nowrap; }
    @container pd-page (max-width: 560px) { .pd-size, .pd-detail { display: none; } .pd-when { width: 6.5rem; } }
  `,
})
export class PdActivityPage {
  protected readonly store = inject(ActivityStore);
  protected readonly heading = $localize`:Page title|@@pd.activity.title:Activity`;
  protected readonly subtitle = $localize`:Page subtitle|@@pd.activity.subtitle:What the app did for you, and what you can take back.`;
  protected readonly whenText = $localize`:Column|@@pd.activity.col.when:When`;
  protected readonly whatText = $localize`:Column|@@pd.activity.col.what:What happened`;
  protected readonly sizeText = $localize`:Column|@@pd.activity.col.size:Size`;
  protected readonly undoText = $localize`:Column|@@pd.activity.col.undo:Undo`;
  protected readonly undoButton = $localize`:Button|@@pd.activity.undo:Undo`;
  protected readonly undoneText = $localize`:State|@@pd.activity.undone:Undone`;
  protected readonly expiredText = $localize`:State|@@pd.activity.expired:No longer possible`;
  protected readonly readFailed = $localize`:Error heading|@@pd.activity.readFailed:The activity log could not be read`;
  protected readonly emptyHeading = $localize`:Empty heading|@@pd.activity.emptyHeading:Nothing yet`;
  protected readonly emptyDescription = $localize`:Empty text|@@pd.activity.emptyDescription:Actions the app takes for you will be listed here.`;

  protected when(e: ActivityEntry): string {
    return formatDateTime(e.atMs);
  }

  protected size(e: ActivityEntry): string {
    return e.bytes === null ? '' : formatBytes(e.bytes);
  }

  protected undoLabel(e: ActivityEntry): string {
    const title = e.title;
    return $localize`:Button name|@@pd.activity.undoNamed:Undo: ${title}:title:`;
  }
}
