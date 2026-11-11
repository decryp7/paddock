import { CdkConnectedOverlay, CdkOverlayOrigin, ConnectedPosition } from '@angular/cdk/overlay';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { formatCount, tzLabel } from '../../core/format';
import { WorkspaceStore } from '../../core/workspace.store';
import { LifecycleService } from '../../lifecycle/lifecycle.service';

/** The styles of the time display and the Details button (one look). */
const TEXT_BUTTON = `
  :host { display: contents; }
  .tz { border: 0; background: transparent; color: inherit; cursor: pointer; padding: 0; font-size: var(--fs-sm); }
  .tz:hover { text-decoration: underline; }
`;

/** Status item (end): the time display, UTC or local; a click switches. */
@Component({
  selector: 'debrief-status-time',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<button class="tz" (click)="store.setTimezone(store.timezone() === 'utc' ? 'local' : 'utc')" title="Toggle UTC / local time" i18n-title="@@shell.statusBar.toggleTimezone">{{ tz() }}</button>`,
  styles: TEXT_BUTTON,
})
export class StatusTime {
  protected readonly store = inject(WorkspaceStore);
  protected readonly tz = computed(() => tzLabel(this.store.timezone()));
}

/** Status item (end): Details, a popover with the rarely needed facts (sources, dropped events, importers, version). */
@Component({
  selector: 'debrief-status-details',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [CdkConnectedOverlay, CdkOverlayOrigin],
  template: `
    <button type="button" class="tz details" data-testid="status-details" cdkOverlayOrigin #origin="cdkOverlayOrigin" aria-haspopup="dialog" [attr.aria-expanded]="detailsOpen()" (click)="detailsOpen.set(!detailsOpen())" i18n="@@shell.statusBar.details">Details</button>
    <ng-template cdkConnectedOverlay [cdkConnectedOverlayOrigin]="origin" [cdkConnectedOverlayOpen]="detailsOpen()" [cdkConnectedOverlayPositions]="positions" cdkConnectedOverlayHasBackdrop cdkConnectedOverlayBackdropClass="cdk-overlay-transparent-backdrop" (backdropClick)="detailsOpen.set(false)" (detach)="detailsOpen.set(false)" (overlayKeydown)="$event.key === 'Escape' && detailsOpen.set(false)">
      <dl class="details-pop" role="dialog" aria-label="Details" i18n-aria-label="@@shell.statusBar.detailsLabel" data-testid="status-details-popover">
        @if (store.summary(); as s) {
          <div><dt i18n="@@shell.statusBar.detail.sources">Sources</dt><dd>{{ fmt(s.sources.length) }}</dd></div>
          <div><dt i18n="@@shell.statusBar.detail.events">Events</dt><dd>{{ fmt(s.total) }}</dd></div>
          @if (s.untimed > 0) {
            <div data-testid="status-untimed" title="Events without a timestamp: in the grid, not on the timeline" i18n-title="@@shell.statusBar.untimedTitle"><dt i18n="@@shell.statusBar.detail.untimed">Without a time</dt><dd>{{ fmt(s.untimed) }}</dd></div>
          }
          @if (s.dropped > 0) {
            <div title="Events removed by the profile pipeline" i18n-title="@@shell.statusBar.droppedTitle"><dt i18n="@@shell.statusBar.detail.dropped">Dropped by pipeline</dt><dd>{{ fmt(s.dropped) }}</dd></div>
          }
          <div><dt i18n="@@shell.statusBar.detail.group">Group</dt><dd>{{ store.effectiveGroupBy().join(', ') || '—' }}</dd></div>
        }
        <div><dt i18n="@@shell.statusBar.detail.importers">Importers</dt><dd>{{ importers() }}</dd></div>
        <div><dt i18n="@@shell.statusBar.detail.version">Version</dt><dd>Debrief {{ version() }}</dd></div>
      </dl>
    </ng-template>
  `,
  styles: `
    ${TEXT_BUTTON}
    .details-pop { margin: 0; min-width: 240px; padding: 8px 12px; border: 1px solid var(--border-3); border-radius: var(--r-3); background: var(--bg-3); color: var(--text); box-shadow: var(--shadow-pop); font-size: var(--fs-sm); display: flex; flex-direction: column; gap: 4px; }
    .details-pop > div { display: flex; justify-content: space-between; gap: 16px; }
    .details-pop dt { color: var(--muted); }
    .details-pop dd { margin: 0; font-variant-numeric: tabular-nums; text-align: end; overflow-wrap: anywhere; }
  `,
})
export class StatusDetails {
  protected readonly store = inject(WorkspaceStore);
  private readonly lifecycle = inject(LifecycleService);
  /** The running app's version, as the backend reports it. */
  protected readonly version = computed(() => this.lifecycle.status()?.version ?? '');
  protected readonly fmt = formatCount;
  protected readonly detailsOpen = signal(false);
  protected readonly positions: ConnectedPosition[] = [{ originX: 'end', originY: 'top', overlayX: 'end', overlayY: 'bottom', offsetY: -4 }];
  protected readonly importers = computed(() => {
    const ids = [...new Set((this.store.summary()?.sources ?? []).map((s) => s.importer))];
    return ids.length ? ids.join(' · ') : '—';
  });
}
