import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatIconModule } from '@angular/material/icon';
import { CONFIRM_DIALOG_ID } from '@decryp7/paddock-ui/shell/confirm';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import type { CommandError, ConnectedApp } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { toCommandError } from '../../core/command-error';
import { Panel } from '../../shared/ui/panel';
import { MCP_KEY_DIALOG_ID } from './mcp-key-dialog';

/**
 * Settings > Connected apps (ADR 0023 Decision 7): the family apps whose tools the Playground chat may use. Connect asks for
 * the app's AI access key in its own window; Rust keeps it, connects and lists the app's tools (read-only ones are asked
 * once, then remembered; actions are asked every call). Refresh lists them again; Forget removes the key, the connection
 * and the remembered approvals. Every decision is Rust's; this page shows the result.
 */
@Component({
  selector: 'pw-connected-apps-page',
  imports: [MatButton, MatIconModule, Panel],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <h2 class="pd-title">Connected apps</h2>
        <p class="help">Apps that lend their tools to the Playground chat. Each tool asks before its first use; actions ask every time.</p>
      </header>
      <pw-panel [loading]="loading()" [ready]="apps() !== null" [error]="error()" (retry)="load()">
        @for (app of apps() ?? []; track app.id) {
          <section class="app" [attr.aria-labelledby]="'pw-app-' + app.id" data-testid="connected-app">
            <div class="row">
              <h3 class="name" [id]="'pw-app-' + app.id">{{ app.title }}</h3>
              <span class="state" [attr.data-state]="stateOf(app)" data-testid="connected-app-state">
                <mat-icon [svgIcon]="app.connected ? 'pw:check' : app.problem ? 'pw:alert' : 'pw:link'" aria-hidden="true" />{{ stateText(app) }}
              </span>
              <span class="spacer"></span>
              <button type="button" matButton="outlined" [disabled]="busy()" (click)="connect(app)">{{ app.keySaved ? 'Replace key…' : 'Connect…' }}</button>
              @if (app.keySaved) {
                @if (!app.keyRefused) {
                  <button type="button" matButton [disabled]="busy()" (click)="refresh(app)">Refresh tools</button>
                }
                <button type="button" matButton [disabled]="busy()" (click)="forget(app)">Forget {{ app.title }}…</button>
              }
            </div>
            @if (app.problem) {
              <p class="problem" role="alert" data-testid="connected-app-problem">{{ app.problem }}</p>
            }
            @if (app.connected) {
              <ul class="tools" [attr.aria-label]="app.title + ' tools'">
                @for (t of app.tools; track t.name) {
                  <li>
                    <span class="tool" [attr.title]="t.name">{{ t.title }}</span>
                    @if (t.remembered) {
                      <span class="kind">Always allowed</span>
                      <button type="button" matButton class="again" [disabled]="busy()" [attr.aria-label]="'Ask again before ' + t.title" (click)="askAgain(app, t.name)">Ask again</button>
                    } @else {
                      <span class="kind">{{ t.readOnly ? 'Reads: asks the first time' : 'Action: asks every time' }}</span>
                    }
                  </li>
                }
              </ul>
              <p class="note">{{ app.remembered === 0 ? 'No tool is always allowed yet.' : app.remembered + (app.remembered === 1 ? ' tool is' : ' tools are') + ' always allowed. Forget the app to ask again.' }}</p>
            }
          </section>
        }
      </pw-panel>
    </div>
  `,
  styles: `
    .page { display: grid; gap: var(--pd-space-3); max-width: 1120px; }
    .page-head { display: block; }
    .pd-title { margin: 0; font-size: var(--pd-text-subtitle); font-weight: var(--pd-weight-semibold); }
    .help { margin: var(--pd-space-1) 0 0; color: var(--pd-text-muted); }
    .app { display: grid; gap: var(--pd-space-2); }
    .row { display: flex; align-items: center; gap: var(--pd-space-2); flex-wrap: wrap; }
    .name { margin: 0; font-size: var(--pd-text-body); font-weight: var(--pd-weight-semibold); }
    .state { display: inline-flex; align-items: center; gap: var(--pd-space-1); color: var(--pd-text-muted); font-size: var(--pd-text-label); }
    .state mat-icon { width: 14px; height: 14px; }
    .state[data-state='connected'] mat-icon { color: var(--pd-success); }
    .state[data-state='problem'] mat-icon { color: var(--pd-warning); }
    .spacer { flex: 1; }
    .problem { margin: 0; color: var(--pd-text); overflow-wrap: anywhere; }
    .tools { list-style: none; margin: 0; padding: 0; display: grid; grid-template-columns: repeat(auto-fill, minmax(220px, 1fr)); gap: var(--pd-space-1) var(--pd-space-4); }
    .tools li { display: flex; align-items: center; gap: var(--pd-space-2); border-bottom: 1px solid var(--pd-hairline); padding: var(--pd-space-1) 0; min-height: 36px; }
    .tools .tool { flex: 1; }
    .tool { font-size: var(--pd-text-body); }
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

  constructor() {
    void this.load();
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
