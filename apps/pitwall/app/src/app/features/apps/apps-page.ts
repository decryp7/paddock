import { Dialog } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { ActivatedRoute } from '@angular/router';
import { firstValueFrom } from 'rxjs';
import type { AppInfo, AppPermissions } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ago, compact, date, duration, int } from '../../shared/format';
import { cacheHitRateText } from '../usage/usage-format';
import { injectNow } from '../../shared/now';
import { Confirm } from '../../shared/ui/confirm';
import { EmptyState } from '../../shared/ui/empty-state';
import { AppAvatar } from '../../shared/ui/app-avatar';
import { ContextMenuService, type ContextMenuItem } from '../../shared/ui/context-menu';
import { Panel } from '../../shared/ui/panel';
import { RowMenu } from '../../shared/ui/row-menu';
import { isMenuKey } from '../../shared/ui/row-nav';
import { limitsText } from '../dashboard/apps-card';
import { groupCode, permissionSummary } from '../dashboard/pairing-card';
import { AppsStore } from './apps.store';
import { CreateAppDialog } from './create-app-dialog';
import { KeySecretDialog, type KeySecretData } from './key-secret-dialog';
import { AppUsage } from '../usage/app-usage';
import { PermissionsEditor } from './permissions-editor';

/** Apps & keys: who may use Pitwall, with which models and how much, their keys and pending pairings. */
@Component({
  selector: 'pw-apps-page',
  imports: [MatIconModule, MatTooltipModule, Panel, EmptyState, PermissionsEditor, AppUsage, AppAvatar, RowMenu],
  providers: [AppsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Apps & keys</h1>
          <p class="sub">Who may use Pitwall, with which models and how much. Pitwall keeps only a hash of each key.</p>
        </div>
        <div class="actions">
          <button type="button" class="btn" [class.primary]="!live.pairings().length" [disabled]="store.isBusy('create')" (click)="newApp()"><mat-icon svgIcon="pw:plus" aria-hidden="true" />New app</button>
        </div>
      </header>

      @if (live.pairings().length) {
        <pw-panel heading="Pairing requests" icon="link" [sub]="live.pairings().length + ' waiting'" class="pairings">
          <ul class="pair-list">
            @for (p of live.pairings(); track p.id) {
              <li>
                <div class="pmain">
                  <p><b>{{ p.appName }}</b>{{ ' ' }}<span class="faint">asked {{ when(p.requestedAt, now()) }}</span></p>
                  <p class="perms" aria-label="Requested permissions">{{ summary(p.permissions).join(' · ') }}</p>
                </div>
                <div class="code" [attr.aria-label]="'Pairing code ' + p.code"><span class="mono">{{ code(p.code) }}</span></div>
                <div class="pact">
                  <button type="button" class="btn ghost" [disabled]="live.isBusy()('pairing:' + p.id)" (click)="live.denyPairing(p.id)">Deny</button>
                  <button type="button" class="btn primary" [disabled]="live.isBusy()('pairing:' + p.id)" (click)="approve(p.id, p.permissions)">
                    <mat-icon svgIcon="pw:check" aria-hidden="true" />Approve
                  </button>
                </div>
              </li>
            }
          </ul>
        </pw-panel>
      }

      @if (store.error() && !store.loaded()) {
        <pw-panel heading="Apps" icon="apps" [error]="store.error()" [ready]="false" (retry)="store.load()" />
      } @else {
        <div class="layout">
          <pw-panel class="list-panel" [flush]="true" heading="Apps" icon="apps" [sub]="store.loaded() ? store.apps().length + '' : ''" [loading]="store.loading()" [ready]="store.loaded()" [error]="store.loaded() ? store.error() : null" (retry)="store.load()">
            @if (store.apps().length === 0) {
              <pw-empty-state icon="apps" heading="No apps yet" text="Apps appear when they pair, or when you create one and give it a key.">
                <button type="button" class="btn sm" (click)="newApp()">New app</button>
              </pw-empty-state>
            }
            <ul class="apps" aria-label="Apps">
              @for (a of store.apps(); track a.id) {
                <li>
                  <button type="button" class="app" [attr.aria-current]="selected()?.id === a.id ? 'true' : null" (click)="select(a.id)">
                    <pw-app-avatar [key]="a.id" [name]="a.name" />
                    <span class="ainfo">
                      <span class="aname"><span class="nm">{{ a.name }}</span></span>
                      <span class="asub num">{{ count(a.today.requests) }} requests today · {{ a.keys.length }} {{ a.keys.length === 1 ? 'key' : 'keys' }}</span>
                    </span>
                    <span class="conn" [class.on]="a.connected" [attr.aria-label]="a.connected ? 'Connected' : 'Not connected'"></span>
                  </button>
                </li>
              }
            </ul>
          </pw-panel>

          <div class="detail">
            @if (selected(); as a) {
              <section class="hero" [attr.aria-label]="a.name">
                <div class="hero-top">
                  <pw-app-avatar [key]="a.id" [name]="a.name" size="lg" />
                  <div class="htitle">
                    <h2>{{ a.name }}</h2>
                    <p>
                      <span class="status" [attr.data-tone]="a.connected ? 'ok' : 'muted'">{{ a.connected ? 'Connected' : 'Not connected' }}</span>
                      <span class="faint">{{ a.connected ? 'active ' : 'last seen ' }}{{ when(a.lastSeenAt, now()) }}</span>
                      <span class="faint lim" [matTooltip]="limits(a.permissions.limits)" matTooltipShowDelay="600">· {{ limits(a.permissions.limits) }}</span>
                    </p>
                  </div>
                  <pw-row-menu [items]="appMenu(a)" [label]="'More actions for ' + a.name" />
                </div>
                <dl class="usage" aria-label="Usage today">
                  <div><dt>Requests</dt><dd class="num">{{ intText(a.today.requests) }}</dd></div>
                  <div><dt>Prompt tokens</dt><dd class="num">{{ count(a.today.promptTokens) }}</dd></div>
                  <div><dt>Completion tokens</dt><dd class="num">{{ count(a.today.completionTokens) }}</dd></div>
                  <div><dt>Busy time</dt><dd class="num">{{ dur(a.today.busyMs) }}</dd></div>
                  <div><dt>Errors</dt><dd class="num" [class.bad]="a.today.errors > 0">{{ a.today.errors }}</dd></div>
                </dl>
                <p class="today faint">Today, since midnight</p>
              </section>

              <pw-panel heading="Cached answers" icon="bolt" [sub]="a.permissions.cacheResponses ? 'on for this app' : 'off for this app'">
                <button panel-actions type="button" class="btn sm" [disabled]="store.isBusy('cache:' + a.id)" (click)="clearAnswers(a)">Clear this app's cached answers</button>
                <dl class="cache-stats" aria-label="Cache use today">
                  <div><dt>Cache hits</dt><dd class="num">{{ intText(a.today.cacheHits) }}</dd></div>
                  <div><dt>Misses</dt><dd class="num">{{ intText(a.today.cacheMisses) }}</dd></div>
                  <div><dt>Hit rate</dt><dd class="num">{{ rateText(a.today) }}</dd></div>
                  <div><dt>Prompt tokens reused</dt><dd class="num">{{ count(a.today.prefixTokens) }}</dd></div>
                </dl>
                <p class="faint cache-note">
                  {{ a.permissions.cacheResponses ? 'Repeated deterministic requests (temperature 0 or a fixed seed, no tools) are answered from the cache.' : 'Turn on Cache answers under Permissions to let repeated deterministic requests reuse a saved answer.' }}
                  Figures are for today.
                </p>
              </pw-panel>

              <pw-app-usage [appId]="a.id" [appName]="a.name" [permissions]="a.permissions" />

              <pw-panel heading="Keys" icon="key" [flush]="true">
                <button panel-actions type="button" class="btn sm" [disabled]="store.isBusy('app:' + a.id)" (click)="createKey(a)"><mat-icon svgIcon="pw:plus" aria-hidden="true" />Create key</button>
                @if (a.keys.length === 0) {
                  <pw-empty-state icon="key" heading="No keys" [text]="a.name + ' cannot call Pitwall until it has a key or pairs again.'" />
                } @else {
                  <table class="table keys fit">
                    <colgroup><col /><col class="c-day" /><col class="c-day" /><col class="c-act" /></colgroup>
                    <thead>
                      <tr>
                        <th scope="col">Key</th>
                        <th scope="col">Created</th>
                        <th scope="col">Last used</th>
                        <th scope="col"><span class="sr-only">Actions</span></th>
                      </tr>
                    </thead>
                    <tbody>
                      @for (k of a.keys; track k.id) {
                        <tr tabindex="0" (keydown)="onKeyRow(a, k, $event)" (contextmenu)="onKeyMenu(a, k, $event)">
                          <td><span class="mono prefix">{{ k.prefix }}</span></td>
                          <td class="muted">{{ day(k.createdAt) }}</td>
                          <td class="muted">{{ when(k.lastUsedAt, now()) }}</td>
                          <td class="right"><pw-row-menu [items]="keyMenu(a, k)" [label]="'More actions for key ' + k.prefix" /></td>
                        </tr>
                      }
                    </tbody>
                  </table>
                }
              </pw-panel>

              <pw-panel heading="Permissions" icon="shield" sub="checked on every request">
                <pw-permissions-editor [permissions]="a.permissions" [models]="store.models()" [appName]="a.name" [busy]="store.isBusy('app:' + a.id)" (save)="savePermissions(a.id, $event)" />
              </pw-panel>
            } @else if (store.loading() && !store.loaded()) {
              <pw-panel [loading]="true" [ready]="false" [skeletonLines]="8" />
            }
          </div>
        </div>
      }
    </div>
  `,
  styles: `
    .page {
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-3);
    }
    .pairings {
      border-color: color-mix(in srgb, var(--pd-accent) 40%, var(--pd-hairline));
      background: linear-gradient(90deg, var(--pd-accent-tint), transparent 60%), var(--pd-surface-1);
    }
    .perms {
      margin: 2px 0 0;
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .pair-list {
      list-style: none;
      margin: 0;
      padding: 0;
      > li {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto auto;
        align-items: center;
        gap: var(--pd-space-4);
        padding: var(--pd-space-3) 0;
        border-top: 1px solid var(--pd-hairline);
        &:first-child {
          border-top: none;
          padding-top: 0;
        }
      }
    }
    .code span {
      display: inline-block;
      padding: var(--pd-space-1) var(--pd-space-3);
      font-size: var(--pd-text-display);
      font-weight: var(--pd-weight-semibold);
      letter-spacing: 0.12em;
      line-height: 1.2;
      background: var(--pd-surface-2);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-control);
    }
    .pact {
      display: flex;
      gap: var(--pd-space-2);
    }
    /* The list beside the app; one column (the list above) when the page is too narrow for both. */
    .page {
      container: apps-page / inline-size;
    }
    .layout {
      display: grid;
      grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
      gap: var(--pd-space-3);
      align-items: start;
    }
    @container apps-page (max-width: 760px) {
      .layout {
        grid-template-columns: minmax(0, 1fr);
      }
      .list-panel {
        position: static;
      }
    }
    .keys {
      .c-day {
        width: 112px;
      }
      .c-act {
        width: 48px;
      }
    }
    .lim {
      min-width: 0;
      overflow: hidden;
      white-space: nowrap;
      text-overflow: ellipsis;
    }
    .list-panel {
      /* conformance: not-a-header */
      position: sticky;
      top: var(--pd-space-4);
    }
    .apps {
      list-style: none;
      margin: 0;
      padding: var(--pd-space-1) var(--pd-space-2) var(--pd-space-2);
    }
    .app {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
      width: 100%;
      padding: var(--pd-space-2);
      font: inherit;
      color: var(--pd-text);
      text-align: left;
      background: none;
      border: none;
      border-radius: var(--pd-radius-control);
      cursor: default;
      &:hover {
        background: var(--pd-hover);
      }
      &[aria-current='true'] {
        background: var(--pd-pressed);
      }
    }
    .ainfo {
      display: flex;
      flex-direction: column;
      flex: 1;
      min-width: 0;
    }
    .aname {
      display: flex;
      align-items: center;
      gap: var(--pd-space-2);
      font-weight: var(--pd-weight-medium);
      .nm {
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
      }
    }
    .asub {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .conn {
      flex: none;
      width: 7px;
      height: 7px;
      border-radius: 50%;
      border: 1.5px solid var(--pd-text-subtle);
      &.on {
        border-color: var(--pd-success);
        background: var(--pd-success);
      }
    }
    .detail {
      display: flex;
      flex-direction: column;
      gap: var(--pd-space-3);
      min-width: 0;
    }
    .hero {
      padding: var(--pd-space-4);
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      box-shadow: var(--pd-shadow-1);
    }
    .hero-top {
      display: flex;
      align-items: center;
      gap: var(--pd-space-3);
    }
    .htitle {
      flex: 1;
      min-width: 0;
      h2 {
        font-size: var(--pd-text-subtitle);
      }
      p {
        display: flex;
        align-items: center;
        gap: var(--pd-space-2);
        margin-top: 4px;
        font-size: var(--pd-text-label);
      }
    }
    /* The day's figures are a row on the card, set off by one hairline: no boxed grid inside the card. */
    .usage {
      display: grid;
      grid-template-columns: repeat(5, minmax(0, 1fr));
      gap: var(--pd-space-4);
      margin: var(--pd-space-4) 0 0;
      padding-top: var(--pd-space-4);
      border-top: 1px solid var(--pd-hairline);
      dt {
        font-size: var(--pd-text-label);
        color: var(--pd-text-muted);
      }
      dd {
        margin: 2px 0 0;
        font-size: var(--pd-text-title);
        font-weight: var(--pd-weight-semibold);
        letter-spacing: -0.01em;
      }
      .bad {
        color: var(--pd-danger);
      }
    }
    .today {
      margin-top: var(--pd-space-2);
      font-size: var(--pd-text-label);
    }
    .cache-stats {
      display: grid;
      grid-template-columns: repeat(4, minmax(0, 1fr));
      gap: var(--pd-space-4);
      margin: 0;
      dt {
        font-size: var(--pd-text-label);
        color: var(--pd-text-muted);
      }
      dd {
        margin: 2px 0 0;
        font-size: var(--pd-text-title);
        font-weight: var(--pd-weight-semibold);
      }
    }
    .cache-note {
      margin: var(--pd-space-3) 0 0;
      font-size: var(--pd-text-label);
    }
    .keys {
      margin-top: var(--pd-space-2);
      th:first-child,
      td:first-child {
        padding-left: var(--pd-space-4);
      }
      th:last-child,
      td:last-child {
        padding-right: var(--pd-space-4);
      }
    }
    .prefix {
      font-size: var(--pd-text-label);
    }
  `,
})
export class AppsPage implements OnInit {
  protected readonly store = inject(AppsStore);
  protected readonly live = inject(LiveStore);
  private readonly dialog = inject(Dialog);
  private readonly confirm = inject(Confirm);
  private readonly menu = inject(ContextMenuService);
  protected readonly now = injectNow(15_000);

  protected readonly when = ago;
  protected readonly day = date;
  protected readonly count = compact;
  protected readonly intText = int;
  protected readonly dur = duration;
  protected readonly code = groupCode;
  protected readonly summary = permissionSummary;
  protected readonly limits = limitsText;
  protected readonly rateText = cacheHitRateText;

  /** `/apps?app=<id>` (from the dashboard's usage legend) opens that app. */
  private readonly selectedId = signal<string | null>(inject(ActivatedRoute, { optional: true })?.snapshot.queryParamMap.get('app') ?? null);
  protected readonly selected = computed<AppInfo | null>(() => {
    const apps = this.store.apps();
    const id = this.selectedId();
    return apps.find((a) => a.id === id) ?? apps[0] ?? null;
  });

  ngOnInit(): void {
    void this.store.load();
  }

  /** Delete is not a button in the page: it lives in this menu and asks first. */
  protected appMenu(a: AppInfo): ContextMenuItem[] {
    return [
      { label: 'Create key', run: () => void this.createKey(a), disabled: this.store.isBusy('app:' + a.id) },
      { label: 'Delete app…', run: () => void this.remove(a), danger: true, divider: true, disabled: this.store.isBusy('app:' + a.id) },
    ];
  }

  /** Revoke is only in the key's menu, in danger text, and names the key. */
  protected keyMenu(a: AppInfo, k: AppInfo['keys'][number]): ContextMenuItem[] {
    return [
      { label: 'Copy key prefix', run: () => void navigator.clipboard?.writeText(k.prefix).catch(() => undefined) },
      { label: 'Revoke key…', run: () => void this.revoke(a, k.id, k.prefix), danger: true, divider: true, disabled: this.store.isBusy('key:' + k.id) },
    ];
  }

  protected onKeyRow(a: AppInfo, k: AppInfo['keys'][number], e: KeyboardEvent): void {
    if (e.target === e.currentTarget && isMenuKey(e)) {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      this.menu.open(r.left + 48, r.bottom - 8, this.keyMenu(a, k));
    }
  }

  protected onKeyMenu(a: AppInfo, k: AppInfo['keys'][number], e: MouseEvent): void {
    e.preventDefault();
    this.menu.open(e.clientX, e.clientY, this.keyMenu(a, k));
  }

  protected select(id: string): void {
    this.selectedId.set(id);
  }

  protected async newApp(): Promise<void> {
    const ref = this.dialog.open<string>(CreateAppDialog, { backdropClass: 'pw-backdrop' });
    const name = await firstValueFrom(ref.closed);
    if (!name) return;
    const app = await this.store.createApp(name);
    if (app) {
      this.selectedId.set(app.id);
      await this.createKey(app);
    }
  }

  protected async createKey(app: AppInfo): Promise<void> {
    const created = await this.store.createKey(app.id);
    if (!created) return;
    const data: KeySecretData = { appName: app.name, created };
    // The secret lives only in this dialog; a stray click on the backdrop must not lose it.
    this.dialog.open(KeySecretDialog, { data, backdropClass: 'pw-backdrop', disableClose: true });
  }

  protected async revoke(app: AppInfo, keyId: string, prefix: string): Promise<void> {
    const ok = await this.confirm.ask({
      title: `Revoke ${prefix}?`,
      message: `${app.name} can no longer use this key: requests with it fail at once. This can't be undone.`,
      confirm: 'Revoke key',
      danger: true,
    });
    if (ok) await this.store.revokeKey(app.id, keyId);
  }

  protected async remove(app: AppInfo): Promise<void> {
    const ok = await this.confirm.ask({
      title: `Delete ${app.name}?`,
      message: `Its keys stop working and its permissions are removed. It can pair again later.`,
      confirm: 'Delete app',
      danger: true,
    });
    if (ok && (await this.store.deleteApp(app.id))) this.selectedId.set(null);
  }

  protected async clearAnswers(app: AppInfo): Promise<void> {
    const ok = await this.confirm.ask({
      title: `Clear ${app.name}'s cached answers?`,
      message: 'Repeated requests are answered by the model again until new answers are cached. Nothing else changes.',
      confirm: 'Clear cached answers',
      danger: true,
    });
    if (ok) await this.store.clearCachedAnswers(app.id);
  }

  protected savePermissions(appId: string, p: AppPermissions): void {
    void this.store.setPermissions(appId, p);
  }

  protected async approve(id: string, p: AppPermissions): Promise<void> {
    await this.live.approvePairing(id, p);
    await this.store.load();
  }
}
