import { ChangeDetectionStrategy, Component, OnInit, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { StatusStore } from './status.store';
import { kindName, lastUpdateText, statusLook } from './status-labels';

/** The main window's first view: every runner's status, versions and last update. */
@Component({
  selector: 'pc-status-view',
  imports: [MatIconModule],
  providers: [StatusStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <header class="bar">
      <h1>Runners</h1>
      <button type="button" class="icon-button" aria-label="Refresh" [disabled]="store.loading()" (click)="store.load()">
        <mat-icon svgIcon="pc:refresh" aria-hidden="true" />
      </button>
    </header>

    @if (store.error(); as error) {
      <div class="error" role="alert">
        <mat-icon svgIcon="pc:alert" aria-hidden="true" />
        <span>{{ error.message }}</span>
        <button type="button" (click)="store.load()">Retry</button>
      </div>
    }

    @if (store.loaded() && store.runners().length === 0) {
      <p class="empty">No runners yet. Add one in Settings.</p>
    }

    <ul class="runners" aria-label="Runners">
      @for (r of store.runners(); track r.name) {
        <li class="runner">
          <mat-icon svgIcon="pc:runner" aria-hidden="true" />
          <div class="main">
            <div class="name">{{ r.name }} <span class="kind">{{ kind(r.kind) }}</span></div>
            <div class="details">
              <span>Version {{ r.currentVersion ?? 'unknown' }}</span>
              @if (r.latestVersion && r.latestVersion !== r.currentVersion) {
                <span>· {{ r.latestVersion }} available</span>
              }
              <span>· {{ lastUpdate(r.lastUpdate) }}</span>
            </div>
          </div>
          <span class="status" [attr.data-tone]="look(r.status).tone">{{ look(r.status).label }}</span>
        </li>
      }
    </ul>
  `,
  styles: `
    :host { display: block; padding: var(--space-4); }
    .bar { display: flex; align-items: center; justify-content: space-between; }
    h1 { font-size: 1.25rem; margin: 0; }
    .icon-button { background: none; border: 1px solid var(--border); border-radius: var(--radius); color: var(--text); padding: var(--space-1); cursor: pointer; display: inline-flex; }
    .icon-button:disabled { opacity: 0.5; cursor: default; }
    .icon-button:focus-visible, button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
    .error { display: flex; gap: var(--space-2); align-items: center; margin-top: var(--space-3); padding: var(--space-2) var(--space-3); border: 1px solid var(--bad); border-radius: var(--radius); color: var(--text); background: var(--bg-2); }
    .error mat-icon { color: var(--bad); }
    .empty { color: var(--text-2); }
    .runners { list-style: none; margin: var(--space-3) 0 0; padding: 0; display: grid; gap: var(--space-2); }
    .runner { display: flex; gap: var(--space-3); align-items: center; padding: var(--space-3); border: 1px solid var(--border); border-radius: var(--radius); background: var(--bg-2); }
    .main { flex: 1; min-width: 0; }
    .name { font-weight: 600; }
    .kind, .details { color: var(--text-2); font-weight: 400; font-size: 0.875rem; }
    .details span { margin-right: var(--space-1); }
    .status { font-size: 0.875rem; padding: 2px var(--space-2); border-radius: 999px; border: 1px solid currentColor; white-space: nowrap; }
    .status[data-tone='ok'] { color: var(--ok); }
    .status[data-tone='busy'] { color: var(--accent); }
    .status[data-tone='warn'] { color: var(--warn); }
    .status[data-tone='bad'] { color: var(--bad); }
    .status[data-tone='muted'] { color: var(--text-2); }
  `,
})
export class StatusView implements OnInit {
  protected readonly store = inject(StatusStore);
  protected readonly look = statusLook;
  protected readonly kind = kindName;
  protected readonly lastUpdate = lastUpdateText;

  ngOnInit(): void {
    void this.store.load();
  }
}
