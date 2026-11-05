import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { MatButton } from '@angular/material/button';
import { MatCheckbox, type MatCheckboxChange } from '@angular/material/checkbox';
import { MatSlideToggle, type MatSlideToggleChange } from '@angular/material/slide-toggle';
import { MatTooltip } from '@angular/material/tooltip';
import { SuEmptyState, SuErrorState, SuSkeleton, SuStatusPill } from '@decryp7/paddock-ui';
import { MCP_KEY_DIALOG_ID, MCP_REVOKE_DIALOG_ID, McpStore, type KeyInfo, type McpView, type ToolInfo } from '@decryp7/paddock-ui/shell/mcp';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { PdMcpAudit } from './mcp-audit';
import { MCP_STYLES } from './styles';

/**
 * Settings > AI access (MCP) (plan X22, ADR 0007): the master switch (off by default), what each tool does and whether it is
 * exposed (a switch per read-only tool; an action tool is a separate, off-by-default permission with a plain warning), the
 * clients' keys (a key is created in the key dialog's window and shown there once; Revoke asks first) and the activity log.
 * Every change is saved at once and the engine applies it to the running server. The page depends on the engine only (rule 19):
 * a failed ask is an inline error with Retry, and a refused change is shown in the engine's words next to what it tried.
 */
@Component({
  selector: 'pd-mcp-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatButton, MatCheckbox, MatSlideToggle, MatTooltip, PdMcpAudit, SuEmptyState, SuErrorState, SuSkeleton, SuStatusPill],
  template: `
    <section aria-labelledby="pd-mc-title" data-testid="mcp-page">
      <h2 id="pd-mc-title">{{ t.title }}</h2>
      <p class="pd-mc__muted">{{ t.lede }}</p>
    </section>

    @if (store.saveError(); as problem) {
      <div class="pd-mc__notice" role="alert" data-testid="mcp-save-error">
        <span>{{ problem }}</span>
        <button matButton="outlined" type="button" data-testid="mcp-save-error-dismiss" (click)="store.dismissSaveError()">{{ t.dismiss }}</button>
      </div>
    }

    @if (store.isLoaded() && store.settings(); as s) {
      <div class="pd-mc__master">
        <mat-slide-toggle [checked]="s.enabled" [disabled]="store.saving()" (change)="apply($event, store.setEnabled)" data-testid="mcp-enabled">
          <span class="pd-mc__label">
            <strong>{{ s.enabled ? t.on : t.off }}</strong>
            <span class="pd-mc__muted">{{ t.masterHint }}</span>
          </span>
        </mat-slide-toggle>
      </div>

      @if (store.listener(); as l) {
        @if (l.kind !== 'unavailable') {
          <section aria-labelledby="pd-mc-conn" data-testid="mcp-connection">
            <h3 id="pd-mc-conn">{{ t.connTitle }}</h3>
            <p [class.pd-mc__warn]="l.kind === 'failed'" data-testid="mcp-listener-state">{{ listenerText(l) }}</p>
            <p class="pd-mc__muted">{{ t.connHow }}</p>
          </section>
        }
      }

      <section aria-labelledby="pd-mc-tools" data-testid="mcp-tools">
        <h3 id="pd-mc-tools">{{ t.toolsTitle }}</h3>
        <p class="pd-mc__muted">{{ t.toolsLede }}</p>
        <ul class="pd-mc__tools">
          @for (tool of store.readOnlyTools(); track tool.name) {
            <li>
              <mat-checkbox class="pd-mc__check" [checked]="!s.disabledTools.includes(tool.name)" [disabled]="store.saving()" (change)="apply($event, store.setToolOn, tool.name)" [attr.data-testid]="'mcp-tool-' + tool.name">
                <span class="pd-mc__label">
                  <strong>{{ label(tool) }}</strong>
                  <span class="pd-mc__muted">{{ tool.description }}</span>
                </span>
              </mat-checkbox>
            </li>
          }
        </ul>
        <mat-checkbox class="pd-mc__check" [checked]="s.resources" [disabled]="store.saving()" (change)="apply($event, store.setResources)" data-testid="mcp-resources">
          <span class="pd-mc__label">
            <strong>{{ t.resources }}</strong>
            <span class="pd-mc__muted">{{ t.resourcesHint }}</span>
          </span>
        </mat-checkbox>
      </section>

      @if (store.actionTools().length > 0) {
        <section aria-labelledby="pd-mc-actions" data-testid="mcp-actions">
          <h3 id="pd-mc-actions">{{ t.actionsTitle }}</h3>
          <p class="pd-mc__warn" data-testid="mcp-actions-warning">{{ t.actionsWarn }}</p>
          <ul class="pd-mc__tools">
            @for (tool of store.actionTools(); track tool.name) {
              <li>
                <mat-checkbox class="pd-mc__check" [checked]="s.allowedActions.includes(tool.name)" [disabled]="store.saving()" (change)="apply($event, store.setActionAllowed, tool.name)" [attr.data-testid]="'mcp-action-' + tool.name">
                  <span class="pd-mc__label">
                    <strong>{{ allowLabel(tool) }}</strong>
                    <span class="pd-mc__muted">{{ tool.description }}</span>
                  </span>
                </mat-checkbox>
              </li>
            }
          </ul>
        </section>
      }

      <section aria-labelledby="pd-mc-clients" data-testid="mcp-clients">
        <div class="pd-mc__head">
          <h3 id="pd-mc-clients">{{ t.clientsTitle }}</h3>
          <button matButton="filled" type="button" data-testid="mcp-add-client" [disabled]="store.atKeyLimit()" disabledInteractive [attr.aria-describedby]="store.atKeyLimit() ? 'pd-mc-limit' : null" (click)="addClient()">{{ t.add }}</button>
        </div>
        <p class="pd-mc__muted">{{ t.clientsLede }}</p>
        @if (store.atKeyLimit()) {
          <p id="pd-mc-limit" class="pd-mc__muted" data-testid="mcp-keys-limit">{{ limitText() }}</p>
        }
        @if (store.keys().length === 0) {
          <pd-empty-state icon="lock" [heading]="t.noClients" [description]="t.noClientsText" data-testid="mcp-no-clients" />
        } @else {
          <table class="pd-mc__table" data-testid="mcp-keys">
            <caption class="pd-sr">{{ t.clientsTitle }}</caption>
            <thead>
              <tr>
                <th scope="col">{{ t.colClient }}</th>
                <th scope="col" class="pd-mc__col-created">{{ t.colCreated }}</th>
                <th scope="col" class="pd-mc__col-state">{{ t.colState }}</th>
                <th scope="col" class="pd-mc__col-actions"><span class="pd-sr">{{ t.colActions }}</span></th>
              </tr>
            </thead>
            <tbody>
              @for (k of store.keys(); track k.id) {
                <tr [attr.data-testid]="'mcp-key-row-' + k.id">
                  <th scope="row"><strong [matTooltip]="k.client">{{ k.client }}</strong></th>
                  <td class="pd-mc__created">{{ created(k) }}</td>
                  <td><pd-status-pill [status]="k.revoked ? 'neutral' : 'success'" [label]="k.revoked ? t.revoked : t.active" /></td>
                  <td>
                    @if (!k.revoked) {
                      <button matButton="outlined" type="button" data-testid="mcp-revoke" [attr.aria-label]="revokeAria(k.client)" (click)="revoke(k)">{{ t.revoke }}</button>
                    }
                  </td>
                </tr>
              }
            </tbody>
          </table>
        }
      </section>
    } @else if (store.requestError(); as problem) {
      <pd-error-state [heading]="t.loadFailed" [cause]="problem" (retry)="store.load()" data-testid="mcp-error" />
    } @else {
      <div class="pd-mc__skeleton" role="status" [attr.aria-label]="t.loading" data-testid="mcp-loading">
        <pd-skeleton shape="block" height="48" />
        <pd-skeleton shape="block" height="36" />
        <pd-skeleton shape="block" height="36" />
      </div>
    }

    <pd-mcp-audit
      [rows]="store.audit()"
      [state]="auditState()"
      [errorText]="store.auditError() ?? ''"
      (clear)="store.clearAudit()"
      (retry)="store.loadAudit()"
    />
  `,
  styles: MCP_STYLES,
})
export class PdMcpPage {
  protected readonly store = inject(McpStore);
  private readonly dialogs = inject(DialogService);

  protected listenerText(l: McpView['listener']): string {
    switch (l.kind) {
      case 'listening':
        return $localize`:@@pd.mcp.conn-listening:Listening on this computer only, port ${l.port}:port:. Clients find it in ${l.discoveryFile}:file:.`;
      case 'failed':
        return $localize`:@@pd.mcp.conn-failed:Not listening: ${l.message}:message:`;
      case 'noKey':
        return this.t.connNoKey;
      default:
        return this.t.connOff;
    }
  }

  protected readonly t = {
    title: $localize`:@@pd.mcp.title:AI access (MCP)`,
    lede: $localize`:@@pd.mcp.lede:Lets a program on this computer, such as a local AI assistant, ask this app questions over the Model Context Protocol. It is off until you turn it on, it only works on this computer, and every call is logged below.`,
    on: $localize`:@@pd.mcp.on:AI access is on`,
    off: $localize`:@@pd.mcp.off:AI access is off`,
    connTitle: $localize`:@@pd.mcp.conn-title:Connection`,
    connOff: $localize`:@@pd.mcp.conn-off:Not listening: AI access is off.`,
    connNoKey: $localize`:@@pd.mcp.conn-no-key:Not listening yet: add a client below to get a key. Nothing listens until a key exists.`,
    connHow: $localize`:@@pd.mcp.conn-how:A client connects with its key while this app is running (it can stay in the tray). Programs that start commands, such as Claude Desktop, run the app's command line with the key in the environment. When the app is quit, clients cannot connect.`,
    masterHint: $localize`:@@pd.mcp.master-hint:Clients connect with a key you give them. Tools are read-only: they look at data and never change it.`,
    toolsTitle: $localize`:@@pd.mcp.tools-title:Tools`,
    toolsLede: $localize`:@@pd.mcp.tools-lede:What a connected client may ask. Switch off what you do not want to share.`,
    resources: $localize`:@@pd.mcp.resources:Resources`,
    resourcesHint: $localize`:@@pd.mcp.resources-hint:Documents the client can read by address, such as a summary.`,
    actionsTitle: $localize`:@@pd.mcp.actions-title:Actions that change things`,
    actionsWarn: $localize`:@@pd.mcp.actions-warn:These tools change something on this computer. They stay off until you allow each one, and a client with a key can then use it without asking you again.`,
    clientsTitle: $localize`:@@pd.mcp.clients-title:Clients`,
    clientsLede: $localize`:@@pd.mcp.clients-lede:Each client has its own key. Revoke a key to cut that client off.`,
    add: $localize`:@@pd.mcp.add:Add a client…`,
    noClients: $localize`:@@pd.mcp.no-clients:No clients yet`,
    noClientsText: $localize`:@@pd.mcp.no-clients-text:Add a client to get a key for it. Nothing can connect without one.`,
    colClient: $localize`:@@pd.mcp.col-client-name:Client`,
    colCreated: $localize`:@@pd.mcp.col-created:Added`,
    colState: $localize`:@@pd.mcp.col-state:State`,
    colActions: $localize`:@@pd.mcp.col-actions:Actions`,
    active: $localize`:@@pd.mcp.active:Active`,
    revoked: $localize`:@@pd.mcp.revoked:Revoked`,
    revoke: $localize`:@@pd.mcp.revoke:Revoke`,
    dismiss: $localize`:@@pd.mcp.dismiss:Dismiss`,
    loadFailed: $localize`:@@pd.mcp.load-failed-heading:The AI access settings could not be read`,
    loading: $localize`:@@pd.mcp.loading:Loading the AI access settings`,
  };

  protected auditState(): 'ready' | 'loading' | 'error' {
    if (this.store.auditError() && this.store.audit().length === 0) return 'error';
    return this.store.auditLoaded() || this.store.audit().length > 0 ? 'ready' : this.store.auditError() ? 'error' : 'loading';
  }

  /**
   * Runs a change that a switch asked for. The browser has already flipped the box; when the engine refuses, the box goes back to
   * what the engine has, so the screen never shows a setting that is not in force.
   */
  protected async apply(event: MatSlideToggleChange | MatCheckboxChange, change: (...args: never[]) => Promise<boolean>, name?: string): Promise<void> {
    const box = event.source;
    const wanted = box.checked;
    const args = (name === undefined ? [wanted] : [name, wanted]) as never[];
    if (!(await change(...args))) box.checked = !wanted;
  }

  protected label(tool: ToolInfo): string {
    return tool.title ?? tool.name;
  }

  protected allowLabel(tool: ToolInfo): string {
    const name = this.label(tool);
    return $localize`:@@pd.mcp.allow:Allow ${name}:tool:`;
  }

  protected limitText(): string {
    const max = this.store.maxKeys();
    return $localize`:@@pd.mcp.keys-limit:${max}:max: keys are in use, the most there can be. Revoke one you no longer need before adding another.`;
  }

  protected created(k: KeyInfo): string {
    return new Date(k.createdMs).toLocaleDateString();
  }

  protected revokeAria(client: string): string {
    return $localize`:@@pd.mcp.revoke-aria:Revoke the key of ${client}:client:`;
  }

  /** Opens the key dialog's window; whatever it did, the list is read again (the key was issued in that other window). */
  protected async addClient(): Promise<void> {
    if (this.store.atKeyLimit()) return;
    await this.dialogs.open<boolean>(MCP_KEY_DIALOG_ID).closed;
    await this.store.refresh();
  }

  protected async revoke(k: KeyInfo): Promise<void> {
    const outcome = await this.dialogs.open<boolean>(MCP_REVOKE_DIALOG_ID, { inputs: { keyId: k.id, client: k.client } }).closed;
    if (outcome.kind === 'answered' && outcome.value === true) await this.store.revoke(k.id);
  }
}
