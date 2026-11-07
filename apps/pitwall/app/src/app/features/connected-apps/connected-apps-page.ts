import { ChangeDetectionStrategy, Component, DestroyRef, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { MatTableModule } from '@angular/material/table';
import { CONFIRM_DIALOG_ID } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import type { AppTool, CommandError, ConnectedApp } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { ItemsMenu, MoreButton, type MenuItem } from '../../shared/ui/menu';
import { Panel } from '../../shared/ui/panel';
import { SettingsHead } from '../../shared/ui/settings-head';
import { MCP_KEY_DIALOG_ID } from './mcp-key-dialog';

/**
 * Settings > Connected apps (ADR 0023 Decision 7): the family apps whose tools the Playground chat may use. Connect asks for
 * the app's AI access key in its own window; Rust keeps it, connects and lists the app's tools (read-only ones are asked
 * once, then remembered; actions are asked every call). Refresh lists them again; Forget removes the key, the connection
 * and the remembered approvals. Every decision is Rust's; this page shows the result.
 */
@Component({
  selector: 'pw-connected-apps-page',
  imports: [MatButton, MatIconModule, MatTableModule, Panel, SettingsHead, ItemsMenu, MoreButton],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <pw-settings-head heading="Connected apps" help="Apps that lend their tools to the Playground chat. Each tool asks before its first use; actions ask every time." />
      <pw-panel [loading]="loading()" [ready]="apps() !== null" [error]="error()" (retry)="load()">
        @for (app of apps() ?? []; track app.id) {
          <section class="app" [attr.aria-labelledby]="'pw-app-' + app.id" data-testid="connected-app">
            <div class="row">
              <h3 class="name" [id]="'pw-app-' + app.id">{{ app.title }}</h3>
              <span class="state" [attr.data-state]="stateOf(app)" data-testid="connected-app-state">
                <mat-icon [svgIcon]="app.connected ? 'pw:check' : app.problem ? 'pw:alert' : 'pw:link'" aria-hidden="true" />{{ stateText(app) }}
              </span>
              <span class="spacer"></span>
              @if (app.keySaved && !app.connected && !app.keyRefused) {
                <button type="button" matButton="outlined" [disabled]="busy() || !!opening()[app.id]" (click)="open(app)">Open {{ app.title }}</button>
              }
              <button type="button" matButton="outlined" [disabled]="busy()" (click)="connect(app)">{{ app.keySaved ? 'Replace key…' : 'Connect…' }}</button>
              @if (app.keySaved) {
                <pw-more-button [menu]="appMenuPanel.menu()" [items]="menuOf(app)" [label]="'More actions for ' + app.title" />
              }
            </div>
            @if (app.problem) {
              <p class="problem" role="alert" data-testid="connected-app-problem">{{ app.problem }}</p>
            }
            @if (opening()[app.id]; as note) {
              <p class="opening" role="status" data-testid="connected-app-opening"><span class="pulse" aria-hidden="true"><i></i><i></i><i></i></span>{{ note }}</p>
            }
            @if (app.connected) {
              <!-- One column of tools, name | access | action (UI review pattern 5: tables, not a grid of cards). -->
              <table mat-table class="tools" [dataSource]="app.tools" [trackBy]="byName" [fixedLayout]="true" [attr.aria-label]="app.title + ' tools'">
                <ng-container matColumnDef="name">
                  <th mat-header-cell *matHeaderCellDef>Tool</th>
                  <td mat-cell *matCellDef="let t" class="name" [attr.title]="t.title + ' (' + t.name + ')'">{{ t.title }}</td>
                </ng-container>
                <ng-container matColumnDef="access">
                  <th mat-header-cell *matHeaderCellDef>Access</th>
                  <td mat-cell *matCellDef="let t" class="kind" [attr.title]="accessOf(t)">{{ accessOf(t) }}</td>
                </ng-container>
                <ng-container matColumnDef="action">
                  <th mat-header-cell *matHeaderCellDef><span class="sr-only">Action</span></th>
                  <td mat-cell *matCellDef="let t" class="action">
                    @if (t.remembered) {
                      <button type="button" matButton class="again" [disabled]="busy()" [attr.aria-label]="'Ask again before ' + t.title" (click)="askAgain(app, t.name)">Ask again</button>
                    }
                  </td>
                </ng-container>
                <tr mat-header-row *matHeaderRowDef="toolColumns"></tr>
                <tr mat-row *matRowDef="let t; columns: toolColumns"></tr>
              </table>
              <p class="note">{{ app.remembered === 0 ? 'No tool is always allowed yet.' : app.remembered + (app.remembered === 1 ? ' tool is' : ' tools are') + ' always allowed.' }}</p>
            }
          </section>
        }
      </pw-panel>
      <pw-items-menu #appMenuPanel label="App actions" />
    </div>
  `,
  styles: `
    .page { display: grid; gap: var(--pd-space-3); max-width: 1120px; }
    .app { display: grid; gap: var(--pd-space-2); }
    .row { display: flex; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; }
    .name { margin: 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
    .state { display: inline-flex; align-items: center; gap: var(--pd-space-1); color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .state mat-icon { width: 14px; height: 14px; }
    .state[data-state='connected'] mat-icon { color: var(--pd-success); }
    .state[data-state='problem'] mat-icon { color: var(--pd-warning); }
    .spacer { flex: 1; }
    .problem { margin: 0; color: var(--pd-text); overflow-wrap: anywhere; }
    /* Name and access share the width; a long one ends in an ellipsis with its full words in a tooltip, never over the next cell. */
    .tools { width: 100%; }
    .tools .mat-column-name { width: 40%; }
    .tools .mat-column-action { width: 120px; text-align: end; }
    .tools td.name, .tools td.kind { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
    .tools td.kind { color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .tools .again { --mat-button-text-container-height: 32px; }
    .opening { margin: 0; display: flex; align-items: center; gap: var(--pd-space-2); color: var(--pd-text-muted); }
    .pulse { display: inline-flex; gap: 3px; }
    .pulse i { width: 5px; height: 5px; border-radius: 50%; background: var(--pd-text-muted); animation: pw-pulse 1.2s ease-in-out infinite; }
    .pulse i:nth-child(2) { animation-delay: 0.2s; }
    .pulse i:nth-child(3) { animation-delay: 0.4s; }
    @keyframes pw-pulse { 0%, 80%, 100% { opacity: 0.25; } 40% { opacity: 1; } }
    @media (prefers-reduced-motion: reduce) { .pulse i { animation: none; opacity: 0.6; } }
    .kind { color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .note { margin: 0; color: var(--pd-text-muted); font-size: var(--pd-text-label); }
  `,
})
export class ConnectedAppsPage {
  private readonly backend = inject(BACKEND);
  private readonly dialogs = inject(DialogService);
  protected readonly apps = signal<ConnectedApp[] | null>(null);
  protected readonly loading = signal(false);
  protected readonly error = signal<CommandError | null>(null);
  protected readonly busy = signal(false);
  /** What "Open <app>" said, per app, until the app connects. */
  protected readonly opening = signal<Record<string, string>>({});

  constructor() {
    void this.load();
    // Rust tells every window about each change (a connect made by the timer after Open, a lost connection).
    const stop = this.backend.onConnectedApps((apps) => {
      this.apps.set(apps);
      const open = { ...this.opening() };
      for (const a of apps) if (a.connected) delete open[a.id];
      this.opening.set(open);
    });
    inject(DestroyRef).onDestroy(stop);
  }

  async load(): Promise<void> {
    this.loading.set(true);
    this.error.set(null);
    try {
      this.apps.set(await this.backend.connectedApps());
    } catch (e) {
      this.error.set(toCommandError(e));
    } finally {
      this.loading.set(false);
    }
  }

  protected stateOf(app: ConnectedApp): 'connected' | 'problem' | 'off' {
    return app.connected ? 'connected' : app.problem ? 'problem' : 'off';
  }

  protected stateText(app: ConnectedApp): string {
    if (app.connected) return `Connected: ${app.tools.length} ${app.tools.length === 1 ? 'tool' : 'tools'}`;
    if (app.problem) return 'Not connected';
    return app.keySaved ? 'Not connected' : 'Not set up';
  }

  private async act(work: () => Promise<ConnectedApp>): Promise<void> {
    this.busy.set(true);
    try {
      const next = await work();
      this.apps.update((list) => (list ?? []).map((a) => (a.id === next.id ? next : a)));
      this.error.set(null);
    } catch (e) {
      this.error.set(toCommandError(e));
    } finally {
      this.busy.set(false);
    }
  }

  protected async connect(app: ConnectedApp): Promise<void> {
    const outcome = await this.dialogs.open<string>(MCP_KEY_DIALOG_ID, { inputs: { app: app.title }, title: `Connect ${app.title}` }).closed;
    if (outcome.kind !== 'answered' || !outcome.value) return;
    const key = outcome.value;
    await this.act(() => this.backend.setAppKey(app.id, key));
  }

  protected refresh(app: ConnectedApp): Promise<void> {
    return this.act(() => this.backend.connectApp(app.id));
  }

  /** Opens the app; the chat connects as soon as it listens (Rust's timer), and the page hears it. */
  protected async open(app: ConnectedApp): Promise<void> {
    this.busy.set(true);
    try {
      const outcome = await this.backend.openConnectedApp(app.id);
      const open = { ...this.opening() };
      // A problem is the row's own (Rust publishes it as the row's problem): nothing is opening, so nothing more is said.
      // Started: it may have connected already (the event can come before this answer), then there is nothing to say.
      const connected = this.apps()?.some((a) => a.id === app.id && a.connected) === true;
      if (outcome.kind === 'started' && !connected) open[app.id] = `Opening ${app.title}…`;
      else delete open[app.id];
      this.opening.set(open);
    } catch (e) {
      this.error.set(toCommandError(e));
    } finally {
      this.busy.set(false);
    }
  }

  /** The row's "…" menu: Refresh tools (not while the key is refused or the app is reconnecting) and Forget. */
  protected menuOf(app: ConnectedApp): MenuItem[] {
    const items: MenuItem[] = [];
    if (!app.keyRefused && !app.reconnecting) items.push({ label: 'Refresh tools', run: () => void this.refresh(app), disabled: this.busy() });
    items.push({ label: `Forget ${app.title}…`, run: () => void this.forget(app), danger: true, divider: items.length > 0, disabled: this.busy() });
    return items;
  }

  /** The tool table's columns. */
  protected readonly toolColumns = ['name', 'access', 'action'];
  protected readonly byName = (_: number, t: AppTool): string => t.name;

  /** How a tool is asked, in words. */
  protected accessOf(t: AppTool): string {
    if (t.remembered) return 'Always allowed';
    return t.readOnly ? 'Reads: asks the first time' : 'Action: asks every time';
  }

  /** Asks again before one tool (its Always allow is forgotten). */
  protected askAgain(app: ConnectedApp, tool: string): Promise<void> {
    return this.act(() => this.backend.revokeToolApproval(app.id, tool));
  }

  /** Forget asks first, naming what goes: the key, the connection and the always-allowed tools. */
  protected async forget(app: ConnectedApp): Promise<void> {
    const what =
      app.remembered === 0
        ? `${app.title}'s key and its connection`
        : `${app.title}'s key, its connection and ${app.remembered === 1 ? 'the tool' : `the ${app.remembered} tools`} you always allowed`;
    const outcome = await this.dialogs.open(CONFIRM_DIALOG_ID, {
      // The window is titled by the page it belongs to; the question is its heading.
      title: 'Connected apps',
      inputs: {
        heading: `Forget ${app.title}?`,
        message: `Pitwall removes ${what}. The chat can't use ${app.title}'s tools until you paste a new key.`,
        confirm: { label: `Forget ${app.title}`, destructive: true },
      },
    }).closed;
    if (outcome.kind !== 'answered') return;
    await this.act(() => this.backend.forgetApp(app.id));
  }
}
