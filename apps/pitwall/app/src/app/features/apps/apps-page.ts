import { Dialog } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, OnInit, computed, inject, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import { firstValueFrom } from 'rxjs';
import type { AppInfo, AppPermissions } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ago, compact, date, duration, int } from '../../shared/format';
import { injectNow } from '../../shared/now';
import { Confirm } from '../../shared/ui/confirm';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { limitsText } from '../dashboard/apps-card';
import { groupCode, permissionSummary } from '../dashboard/pairing-card';
import { AppsStore } from './apps.store';
import { CreateAppDialog } from './create-app-dialog';
import { KeySecretDialog, type KeySecretData } from './key-secret-dialog';
import { PermissionsEditor } from './permissions-editor';

/** Apps & keys: who may use Pitwall, with which models and how much, their keys and pending pairings. */
@Component({
  selector: 'pw-apps-page',
  imports: [MatIconModule, MatTooltipModule, Panel, EmptyState, PermissionsEditor],
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
          <button type="button" class="btn primary" [disabled]="store.isBusy('create')" (click)="newApp()"><mat-icon svgIcon="pw:plus" aria-hidden="true" />New app</button>
        </div>
      </header>

      @if (live.pairings().length) {
        <pw-panel heading="Pairing requests" icon="link" [sub]="live.pairings().length + ' waiting'" class="pairings">
          <ul class="pair-list">
            @for (p of live.pairings(); track p.id) {
              <li>
                <div class="pmain">
                  <p><b>{{ p.appName }}</b> <span class="faint">asked {{ when(p.requestedAt, now()) }}</span></p>
                  <ul class="tags" aria-label="Requested permissions">
                    @for (t of summary(p.permissions); track t) {
                      <li class="tag">{{ t }}</li>
                    }
                  </ul>
                </div>
                <div class="code" [attr.aria-label]="'Pairing code ' + p.code"><span class="mono">{{ code(p.code) }}</span></div>
                <div class="pact">
                  <button type="button" class="btn" [disabled]="live.isBusy()('pairing:' + p.id)" (click)="live.denyPairing(p.id)">Deny</button>
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
                    <span class="avatar" aria-hidden="true">{{ a.name[0] }}</span>
                    <span class="ainfo">
                      <span class="aname">
                        <span class="nm">{{ a.name }}</span>
                        @if (a.suite) {
                          <span class="tag">Suite</span>
                        }
                      </span>
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
                  <span class="avatar lg" aria-hidden="true">{{ a.name[0] }}</span>
                  <div class="htitle">
                    <h2>{{ a.name }}</h2>
                    <p>
                      <span class="pill dot" [attr.data-tone]="a.connected ? 'ok' : 'muted'">{{ a.connected ? 'Connected' : 'Not connected' }}</span>
                      <span class="faint">{{ a.connected ? 'active ' : 'last seen ' }}{{ when(a.lastSeenAt, now()) }}</span>
                      <span class="faint">· {{ limits(a.permissions.limits) }}</span>
                    </p>
                  </div>
                  <button type="button" class="btn ghost danger sm" [disabled]="store.isBusy('app:' + a.id)" (click)="remove(a)"><mat-icon svgIcon="pw:trash" aria-hidden="true" />Delete app</button>
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

              <pw-panel heading="Keys" icon="key" [sub]="a.keys.length ? a.keys.length + ' active' : ''" [flush]="true">
                <button panel-actions type="button" class="btn sm" [disabled]="store.isBusy('app:' + a.id)" (click)="createKey(a)"><mat-icon svgIcon="pw:plus" aria-hidden="true" />Create key</button>
                @if (a.keys.length === 0) {
                  <pw-empty-state icon="key" heading="No keys" [text]="a.name + ' cannot call Pitwall until it has a key or pairs again.'" />
                } @else {
                  <table class="table keys">
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
                        <tr>
                          <td><span class="mono prefix">{{ k.prefix }}</span></td>
                          <td class="muted">{{ day(k.createdAt) }}</td>
                          <td class="muted">{{ when(k.lastUsedAt, now()) }}</td>
                          <td class="right">
                            <button type="button" class="btn ghost sm danger" [attr.aria-label]="'Revoke key ' + k.prefix" [disabled]="store.isBusy('key:' + k.id)" (click)="revoke(a, k.id, k.prefix)">Revoke</button>
                          </td>
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
      gap: var(--sp-3);
    }
    .pairings {
      border-color: color-mix(in srgb, var(--accent) 40%, var(--border));
      background: linear-gradient(90deg, var(--accent-soft), transparent 60%), var(--bg-card);
    }
    .pair-list {
      list-style: none;
      margin: 0;
      padding: 0;
      > li {
        display: grid;
        grid-template-columns: minmax(0, 1fr) auto auto;
        align-items: center;
        gap: var(--sp-4);
        padding: var(--sp-3) 0;
        border-top: 1px solid var(--border-soft);
        &:first-child {
          border-top: none;
          padding-top: 0;
        }
      }
    }
    .tags {
      display: flex;
      flex-wrap: wrap;
      gap: var(--sp-1);
      margin: var(--sp-2) 0 0;
      padding: 0;
      list-style: none;
    }
    .code span {
      display: inline-block;
      padding: var(--sp-1) var(--sp-3);
      font-size: 18px;
      font-weight: var(--fw-semibold);
      letter-spacing: 0.08em;
      background: var(--bg-sunken);
      border: 1px solid var(--border);
      border-radius: var(--r-md);
    }
    .pact {
      display: flex;
      gap: var(--sp-2);
    }
    .layout {
      display: grid;
      grid-template-columns: 300px minmax(0, 1fr);
      gap: var(--sp-3);
      align-items: start;
    }
    .list-panel {
      position: sticky;
      top: var(--sp-4);
    }
    .apps {
      list-style: none;
      margin: 0;
      padding: var(--sp-1) var(--sp-2) var(--sp-2);
    }
    .app {
      display: flex;
      align-items: center;
      gap: var(--sp-3);
      width: 100%;
      padding: var(--sp-2);
      font: inherit;
      color: var(--text);
      text-align: left;
      background: none;
      border: none;
      border-radius: var(--r-md);
      cursor: pointer;
      &:hover {
        background: var(--bg-hover);
      }
      &[aria-current='true'] {
        background: var(--bg-active);
        .avatar {
          color: var(--text-on-accent);
          background: var(--accent);
        }
      }
    }
    .avatar {
      display: grid;
      place-items: center;
      flex: none;
      width: 28px;
      height: 28px;
      font-size: var(--fs-sm);
      font-weight: var(--fw-semibold);
      color: var(--text-2);
      background: var(--bg-active);
      border-radius: var(--r-md);
      &.lg {
        width: 40px;
        height: 40px;
        font-size: var(--fs-lg);
        color: var(--text-on-accent);
        background: var(--accent);
        border-radius: var(--r-lg);
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
      gap: var(--sp-2);
      font-weight: var(--fw-medium);
      .nm {
        white-space: nowrap;
        overflow: hidden;
        text-overflow: ellipsis;
      }
    }
    .asub {
      font-size: var(--fs-xs);
      color: var(--text-3);
    }
    .conn {
      flex: none;
      width: 7px;
      height: 7px;
      border-radius: 50%;
      border: 1.5px solid var(--text-3);
      &.on {
        border-color: var(--ok);
        background: var(--ok);
      }
    }
    .detail {
      display: flex;
      flex-direction: column;
      gap: var(--sp-3);
      min-width: 0;
    }
    .hero {
      padding: var(--sp-5);
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-radius: var(--r-lg);
      box-shadow: var(--shadow-1), var(--card-highlight);
    }
    .hero-top {
      display: flex;
      align-items: center;
      gap: var(--sp-3);
    }
    .htitle {
      flex: 1;
      min-width: 0;
      h2 {
        font-size: var(--fs-lg);
      }
      p {
        display: flex;
        align-items: center;
        gap: var(--sp-2);
        margin-top: 4px;
        font-size: var(--fs-sm);
      }
    }
    .usage {
      display: grid;
      grid-template-columns: repeat(5, minmax(0, 1fr));
      gap: 1px;
      margin: var(--sp-5) 0 0;
      background: var(--border-soft);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-md);
      overflow: hidden;
      > div {
        padding: var(--sp-3);
        background: var(--bg-card);
      }
      dt {
        font-size: var(--fs-xs);
        color: var(--text-3);
      }
      dd {
        margin: 2px 0 0;
        font-size: var(--fs-xl);
        font-weight: var(--fw-semibold);
        letter-spacing: -0.01em;
      }
      .bad {
        color: var(--bad);
      }
    }
    .today {
      margin-top: var(--sp-2);
      font-size: var(--fs-xs);
    }
    .keys {
      margin-top: var(--sp-2);
      th:first-child,
      td:first-child {
        padding-left: var(--sp-4);
      }
      th:last-child,
      td:last-child {
        padding-right: var(--sp-4);
      }
    }
    .prefix {
      padding: 2px 6px;
      background: var(--bg-sunken);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-xs);
    }
  `,
})
export class AppsPage implements OnInit {
  protected readonly store = inject(AppsStore);
  protected readonly live = inject(LiveStore);
  private readonly dialog = inject(Dialog);
  private readonly confirm = inject(Confirm);
  protected readonly now = injectNow(15_000);

  protected readonly when = ago;
  protected readonly day = date;
  protected readonly count = compact;
  protected readonly intText = int;
  protected readonly dur = duration;
  protected readonly code = groupCode;
  protected readonly summary = permissionSummary;
  protected readonly limits = limitsText;

  private readonly selectedId = signal<string | null>(null);
  protected readonly selected = computed<AppInfo | null>(() => {
    const apps = this.store.apps();
    const id = this.selectedId();
    return apps.find((a) => a.id === id) ?? apps[0] ?? null;
  });

  ngOnInit(): void {
    void this.store.load();
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

  protected savePermissions(appId: string, p: AppPermissions): void {
    void this.store.setPermissions(appId, p);
  }

  protected async approve(id: string, p: AppPermissions): Promise<void> {
    await this.live.approvePairing(id, p);
    await this.store.load();
  }
}
