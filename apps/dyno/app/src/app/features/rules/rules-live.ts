import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import type { ActiveAction } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { formatRelative } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { ActivityStore } from '../../state/activity.store';
import { LiveStore } from '../../state/live.store';
import { RulesStore } from '../../state/rules.store';

/** What rules are doing right now (with a Release per app) and the last few things they did. */
@Component({
  selector: 'dy-rules-live',
  imports: [AsyncState, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <section class="card" aria-label="In effect now">
      <div class="card-head"><h2>In effect now</h2></div>
      @if (actions().length === 0) {
        <div class="empty"><dy-icon name="shield-check" /><strong>Nothing is being throttled</strong><span>When a rule steps in, the app shows up here and you can release it.</span></div>
      } @else {
        <table class="table">
          <caption class="visually-hidden">What Dyno is doing to apps right now</caption>
          <thead><tr><th>App</th><th>Rule</th><th>What Dyno did</th><th>Since</th><th class="act"><span class="visually-hidden">Actions</span></th></tr></thead>
          <tbody>
            @for (a of actions(); track a.target.pid) {
              <tr>
                <td class="app"><span class="truncate" [title]="a.app">{{ a.app }}</span></td>
                <td class="muted-cell"><span class="truncate" [title]="a.ruleName">{{ a.ruleName }}</span></td>
                <td>{{ a.description }}</td>
                <td class="muted-cell">{{ since(a) }}</td>
                <td class="act"><button type="button" class="btn ghost sm" [attr.aria-label]="'Release ' + a.app" (click)="rules.release(a.target)"><dy-icon name="undo" /> Release</button></td>
              </tr>
            }
          </tbody>
        </table>
      }
    </section>

    <section class="card" aria-label="Recent activity">
      <div class="card-head"><h2>Recent activity</h2></div>
      <dy-async [loaded]="activity.loaded()" [error]="activity.error()" [empty]="recent().length === 0" emptyTitle="No rule activity yet" emptyText="What rules do, and what you release, is listed here." emptyIcon="clock" what="recent activity" [rows]="3" (retry)="activity.load()">
        <table class="table">
          <caption class="visually-hidden">Recent rule activity</caption>
          <thead><tr><th>When</th><th>What happened</th><th class="act"><span class="visually-hidden">Actions</span></th></tr></thead>
          <tbody>
            @for (e of recent(); track e.id) {
              <tr>
                <td class="when muted-cell">{{ ago(e.at) }}</td>
                <td><strong>{{ e.title }}</strong>@if (e.detail) { <span class="muted-cell"> {{ e.detail }}</span> }</td>
                <td class="act">
                  @if (e.undo === 'available') {
                    <button type="button" class="btn ghost sm" [disabled]="activity.undoing().includes(e.id)" [attr.aria-label]="'Undo: ' + e.title" (click)="activity.undo(e.id)"><dy-icon name="undo" /> Undo</button>
                  } @else if (e.undo === 'undone') {
                    <span class="muted-cell">Undone</span>
                  }
                </td>
              </tr>
            }
          </tbody>
        </table>
      </dy-async>
    </section>
  `,
  styles: `
    :host { display: contents; }
    .card-head { padding: var(--sp-3) var(--sp-4) var(--sp-2); }
    .card-head h2 { font-size: var(--fs-md); }
    .table { width: 100%; }
    .table th:first-child, .table td:first-child { padding-left: var(--sp-4); }
    .table td { max-width: 320px; overflow: hidden; text-overflow: ellipsis; }
    .table .act { width: 1%; text-align: right; padding-right: var(--sp-2); }
    .table .when { width: 1%; }
    .app { font-weight: var(--fw-medium); }
    .truncate { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .muted-cell { color: var(--text-3); }
    .empty { display: flex; align-items: center; gap: var(--sp-3); padding: var(--sp-4); color: var(--text-2); }
    .empty dy-icon { --icon-size: 20px; color: var(--ok); }
    .empty strong { color: var(--text-1); font-weight: var(--fw-medium); }
    .empty span { color: var(--text-3); font-size: var(--fs-sm); }
    tbody tr:last-child td { border-bottom: 0; }
  `,
})
export class RulesLive implements OnInit {
  protected readonly rules = inject(RulesStore);
  protected readonly activity = inject(ActivityStore);
  private readonly live = inject(LiveStore);
  protected readonly actions = computed<ActiveAction[]>(() => this.live.update()?.activeActions ?? []);
  protected readonly recent = computed(() => this.activity.entries().filter((e) => e.kind === 'rule' || e.kind === 'release').slice(0, 10));

  ngOnInit(): void {
    if (!this.activity.loaded()) void this.activity.load();
  }

  protected since = (a: ActiveAction): string => formatRelative(a.since, Date.now());
  protected ago = (at: number): string => formatRelative(at, Date.now());
}
