import { ChangeDetectionStrategy, Component, OnInit, computed, inject } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatSlideToggleModule } from '@angular/material/slide-toggle';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { PluginKind } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { pluginStateLook } from '../../shared/labels';
import { EmptyState } from '../../shared/ui/empty-state';
import { Panel } from '../../shared/ui/panel';
import { PluginsStore } from './plugins.store';

export const KIND_LABEL: Record<PluginKind, string> = {
  engine: 'Engine',
  endpoint: 'Endpoint',
  tool: 'Tool',
  filter: 'Filter',
};

/** Plugins & tools: out-of-process plugins (guarded, quarantined after repeated failures) and MCP tool servers. */
@Component({
  selector: 'pw-plugins-page',
  imports: [MatIconModule, MatSlideToggleModule, MatTooltipModule, Panel, EmptyState],
  providers: [PluginsStore],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div>
          <h1>Plugins & tools</h1>
          <p class="sub">Plugins run in their own process with time and memory limits; a crash never takes Pitwall down.</p>
        </div>
        <div class="actions">
          <button type="button" class="btn" [disabled]="store.loading()" (click)="store.load()"><mat-icon svgIcon="pw:refresh" aria-hidden="true" />Refresh</button>
        </div>
      </header>

      <pw-panel heading="Plugins" icon="plugins" [sub]="store.loaded() && !store.pluginsError() ? store.plugins().length + ' installed' : ''" [flush]="true" [loading]="store.loading()" [ready]="store.loaded() && (!store.pluginsError() || store.plugins().length > 0)" [error]="store.pluginsError()" [skeletonLines]="6" (retry)="store.load()">
        @if (store.plugins().length === 0) {
          <pw-empty-state icon="plugins" heading="No plugins installed" text="Plugins add engines, endpoints, tools and filters. Install one by copying it into Pitwall's plugins folder." />
        }
        <ul class="plugins" aria-label="Plugins">
          @for (p of store.plugins(); track p.id) {
            <li [class.quarantined]="p.state === 'quarantined'">
              <div class="ic" aria-hidden="true"><mat-icon svgIcon="pw:plugins" /></div>
              <div class="main">
                <div class="title">
                  <span class="name">{{ p.name }}</span>
                  <span class="ver mono">{{ p.version }}</span>
                  <span class="pill dot" [attr.data-tone]="look(p.state).tone">{{ look(p.state).label }}</span>
                </div>
                <div class="meta">
                  @for (k of p.kinds; track k) {
                    <span class="tag">{{ kind[k] }}</span>
                  }
                  <span class="net" [class.allowed]="p.network">
                    <mat-icon [svgIcon]="p.network ? 'pw:globe' : 'pw:lock'" aria-hidden="true" />Network: {{ p.network ? 'allowed by you' : 'never' }}
                  </span>
                  @if (p.failures > 0) {
                    <span class="faint num">{{ p.failures }} {{ p.failures === 1 ? 'failure' : 'failures' }}</span>
                  }
                </div>
                @if (p.state === 'quarantined' || p.state === 'failed') {
                  <div class="callout" role="note">
                    <mat-icon svgIcon="pw:alert" aria-hidden="true" />
                    <div>
                      <p>
                        @if (p.state === 'quarantined') {
                          <b>Quarantined after repeated failures.</b> Pitwall stopped calling it to keep everything else running.
                        } @else {
                          <b>It failed to start.</b>
                        }
                      </p>
                      @if (p.lastError) {
                        <p class="mono last">{{ p.lastError }}</p>
                      }
                      @if (p.state === 'quarantined') {
                        <p class="faint">Turn it off and on again to give it another chance.</p>
                      }
                    </div>
                  </div>
                }
              </div>
              <mat-slide-toggle class="toggle" [checked]="p.enabled" [disabled]="store.isBusy(p.id)" (change)="store.setEnabled(p.id, $event.checked)" [attr.aria-label]="(p.enabled ? 'Disable ' : 'Enable ') + p.name" />
            </li>
          }
        </ul>
      </pw-panel>

      <pw-panel heading="MCP servers" icon="tool" [sub]="store.loaded() && !store.mcpError() ? store.mcp().length + ' configured' : ''" [flush]="true" [loading]="store.loading()" [ready]="store.loaded() && (!store.mcpError() || store.mcp().length > 0)" [error]="store.mcpError()" [skeletonLines]="4" (retry)="store.load()">
        @if (store.mcp().length === 0) {
          <pw-empty-state icon="tool" heading="No MCP servers" text="MCP servers give models tools such as reading files. Apps use them only when you allow it." />
        }
        <ul class="mcp" aria-label="MCP servers">
          @for (m of store.mcp(); track m.id) {
            <li>
              <div class="mtop">
                <span class="name">{{ m.name }}</span>
                <span class="pill dot" [attr.data-tone]="look(m.state).tone">{{ look(m.state).label }}</span>
              </div>
              <code class="cmd" [matTooltip]="m.command">{{ m.command }}</code>
              <dl>
                <dt>Tools</dt>
                <dd>
                  @for (t of m.tools; track t) {
                    <span class="tag mono">{{ t }}</span>
                  } @empty {
                    <span class="faint">None reported</span>
                  }
                </dd>
                <dt>Allowed apps</dt>
                <dd>
                  @for (a of m.allowedApps; track a) {
                    <span class="tag">{{ appName(a) }}</span>
                  } @empty {
                    <span class="faint">No app may use it yet</span>
                  }
                </dd>
              </dl>
            </li>
          }
        </ul>
      </pw-panel>
    </div>
  `,
  styles: `
    .page {
      display: flex;
      flex-direction: column;
      gap: var(--sp-3);
    }
    .page-head {
      margin-bottom: var(--sp-2);
    }
    ul {
      list-style: none;
      margin: 0;
      padding: 0;
    }
    .plugins > li {
      display: grid;
      grid-template-columns: auto minmax(0, 1fr) auto;
      gap: var(--sp-3);
      align-items: start;
      padding: var(--sp-4);
      border-top: 1px solid var(--border-soft);
      &:first-child {
        border-top: none;
      }
    }
    .ic {
      display: grid;
      place-items: center;
      width: 32px;
      height: 32px;
      color: var(--text-2);
      background: var(--bg-hover);
      border-radius: var(--r-md);
      mat-icon {
        width: 16px;
        height: 16px;
      }
    }
    .quarantined .ic {
      color: var(--bad);
      background: var(--bad-soft);
    }
    .title {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      min-height: 22px;
    }
    .name {
      font-weight: var(--fw-medium);
    }
    .ver {
      font-size: var(--fs-xs);
      color: var(--text-3);
    }
    .meta {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: var(--sp-2);
      margin-top: 6px;
      font-size: var(--fs-xs);
    }
    .net {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      color: var(--text-2);
      mat-icon {
        width: 12px;
        height: 12px;
        color: var(--ok);
      }
      &.allowed mat-icon {
        color: var(--warn);
      }
    }
    .callout {
      display: flex;
      gap: var(--sp-2);
      margin-top: var(--sp-3);
      padding: var(--sp-3);
      font-size: var(--fs-sm);
      background: var(--bad-soft);
      border-radius: var(--r-md);
      > mat-icon {
        flex: none;
        color: var(--bad);
        margin-top: 1px;
      }
      p + p {
        margin-top: 4px;
      }
      .last {
        font-size: var(--fs-xs);
        color: var(--text-2);
      }
    }
    .toggle {
      margin-top: 4px;
    }
    .mcp {
      display: grid;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: var(--sp-3);
      padding: var(--sp-4);
      > li {
        padding: var(--sp-4);
        background: var(--bg-sunken);
        border: 1px solid var(--border-soft);
        border-radius: var(--r-md);
        min-width: 0;
      }
    }
    .mtop {
      display: flex;
      justify-content: space-between;
      align-items: center;
      gap: var(--sp-2);
    }
    .cmd {
      display: block;
      margin-top: var(--sp-2);
      padding: 6px var(--sp-2);
      font-size: var(--fs-xs);
      color: var(--text-2);
      background: var(--bg-card);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-sm);
      white-space: nowrap;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    dl {
      display: grid;
      grid-template-columns: 96px 1fr;
      gap: var(--sp-2) var(--sp-3);
      margin: var(--sp-3) 0 0;
      font-size: var(--fs-sm);
      dt {
        color: var(--text-3);
        font-size: var(--fs-xs);
        padding-top: 2px;
      }
      dd {
        display: flex;
        flex-wrap: wrap;
        gap: var(--sp-1);
        margin: 0;
      }
    }
  `,
})
export class PluginsPage implements OnInit {
  protected readonly store = inject(PluginsStore);
  private readonly live = inject(LiveStore);
  protected readonly look = pluginStateLook;
  protected readonly kind = KIND_LABEL;
  private readonly names = computed(() => new Map(this.live.apps().map((a) => [a.id, a.name])));

  ngOnInit(): void {
    void this.store.load();
  }

  protected appName(id: string): string {
    return this.names().get(id) ?? id;
  }
}
