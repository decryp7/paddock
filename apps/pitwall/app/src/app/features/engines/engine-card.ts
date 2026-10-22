import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { BuildCapability, Engine, InstalledBuild } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ago, bytes, percent } from '../../shared/format';
import { availabilityLook, capabilityLook, downloadActive, healthLook, selfTestLook } from '../../shared/labels';
import { Confirm } from '../../shared/ui/confirm';
import { EmptyState } from '../../shared/ui/empty-state';
import { EnginesStore } from './engines.store';

/** A readable model name from its id, for the process table. */
export function modelLabel(id: string): string {
  return id.replace(/[-.](q\d\w*|f16|bf16|f32)$/i, '');
}

/** Engine downloads are named `<engine> <build>` (llama.cpp builds and pack engines alike): each card shows only its own. */
export function ownsDownload(engineName: string, downloadName: string): boolean {
  return downloadName.startsWith(`${engineName} `);
}

/** An engine that can serve: its build choice, rollback, installed builds, the pinned catalog and its processes. */
@Component({
  selector: 'pw-engine-card',
  imports: [MatIconModule, MatTooltipModule, EmptyState],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let e = engine();
    <section class="card" [attr.aria-labelledby]="'eng-' + e.kind">
      <header class="head">
        <div class="logo" aria-hidden="true"><mat-icon svgIcon="pw:engines" /></div>
        <div class="title">
          <h2 [id]="'eng-' + e.kind">{{ e.name }}</h2>
          <p class="muted">{{ e.description }}</p>
        </div>
        <span class="pill dot" [attr.data-tone]="look().tone">{{ look().label }}</span>
      </header>

      <div class="choice">
        <div class="field">
          <label for="build-choice">Build</label>
          <select id="build-choice" class="select" [disabled]="store.isBusy()('select') || e.builds.length === 0" (change)="store.select($any($event.target).value)">
            <option value="auto" [selected]="e.buildChoice === 'auto'">Automatic{{ activeBuild() ? ' — uses ' + activeBuild()!.label : '' }}</option>
            @for (b of e.builds; track b.id + b.tag) {
              <option [value]="b.id" [selected]="e.buildChoice === b.id" [disabled]="b.selfTest === 'failed'">{{ b.label }} · {{ b.tag }}</option>
            }
          </select>
        </div>
        <p class="hint">Automatic picks the fastest installed build that passed its self-test on this hardware.</p>
        @if (rollbackTarget(); as t) {
          <button type="button" class="btn" [disabled]="store.isBusy()('rollback')" (click)="rollback()"><mat-icon svgIcon="pw:rollback" aria-hidden="true" />Roll back to {{ t.label }} {{ t.tag }}</button>
        } @else if (rollbackBlocked(); as why) {
          <p class="hint" data-test="rollback-blocked">Roll back is not available: {{ why }}</p>
        }
      </div>

      @if (pinnedLabel(); as p) {
        <p class="pinned-note" role="status"><mat-icon svgIcon="pw:pin" aria-hidden="true" />{{ p }}</p>
      }
      @for (problem of problems(); track problem) {
        <p class="pin-problem" role="alert">{{ problem }}</p>
      }

      <h3>Installed builds</h3>
      @if (e.builds.length === 0) {
        <pw-empty-state icon="engines" heading="No build installed" text="Install one from the catalog below; the suggested one suits this machine." />
      } @else {
        <table class="table">
          <thead>
            <tr>
              <th scope="col">Build</th>
              <th scope="col">Release</th>
              <th scope="col" class="right">Size</th>
              <th scope="col">Self-test</th>
              <th scope="col">Devices</th>
              @if (canPin()) {
                <th scope="col">Version pin</th>
              }
              <th scope="col"><span class="sr-only">Actions</span></th>
            </tr>
          </thead>
          <tbody>
            @for (b of e.builds; track b.id + b.tag) {
              <tr>
                <td>
                  <span class="bname">{{ b.label }}</span>
                  @if (b.active) {
                    <span class="pill" data-tone="info">Active</span>
                  }
                </td>
                <td>
                  <span class="mono">{{ b.tag }}</span>
                  @if (b.outdated) {
                    <span class="tag upd" matTooltip="A newer pinned release is in the catalog">update</span>
                  }
                </td>
                <td class="right num">{{ size(b.sizeBytes) }}</td>
                <td><span class="pill dot" [attr.data-tone]="test(b).tone" [matTooltip]="b.selfTestMessage ?? ''">{{ test(b).label }}</span></td>
                <td class="devices" [matTooltip]="b.devices.join(', ')">{{ b.devices.join(', ') || '—' }}</td>
                @if (canPin()) {
                  <td>
                    <button
                      type="button"
                      class="btn sm pin"
                      [class.pinned]="isPinned(b)"
                      [attr.aria-pressed]="isPinned(b)"
                      [attr.aria-label]="(isPinned(b) ? 'Unpin ' : 'Pin ') + b.label + ' ' + b.tag + ' for all models'"
                      [matTooltip]="isPinned(b) ? 'Pinned for all models: automatic choice is off. Click to unpin.' : b.selfTest === 'passed' ? 'Use this build for all models, even when a better one is installed' : 'Only builds that passed their self-test can be pinned'"
                      [disabled]="store.isBusy()('pin') || (!isPinned(b) && b.selfTest !== 'passed')"
                      (click)="store.togglePin(b)"
                    >
                      <mat-icon [svgIcon]="isPinned(b) ? 'pw:pin' : 'pw:unpin'" aria-hidden="true" />{{ isPinned(b) ? 'Pinned' : 'Pin' }}
                    </button>
                  </td>
                }
                <td class="right">
                  <button type="button" class="btn ghost sm caps-toggle" [attr.aria-expanded]="capsOpen().has(b.id)" [attr.aria-controls]="'caps-' + b.id" [attr.aria-label]="'What ' + b.label + ' ' + b.tag + ' supports'" (click)="toggleCaps(b.id)">
                    Features
                  </button>
                  <button type="button" class="btn ghost icon sm del" [attr.aria-label]="'Remove ' + b.label + ' ' + b.tag" [matTooltip]="b.active ? 'In use: choose another build first' : 'Remove'" [disabled]="b.active || store.isBusy()('remove:' + b.id + ':' + b.tag)" (click)="remove(b)">
                    <mat-icon svgIcon="pw:trash" aria-hidden="true" />
                  </button>
                </td>
              </tr>
              @if (capsOpen().has(b.id)) {
                <tr class="caps-row">
                  <td [id]="'caps-' + b.id" [attr.colspan]="canPin() ? 7 : 6">
                    @if (capsOf(b); as list) {
                      <ul class="caps" [attr.aria-label]="'Features of ' + b.label + ' ' + b.tag">
                        @for (c of list; track c.capability) {
                          <li>
                            <span class="pill dot" [attr.data-tone]="capLook(c).tone">{{ capLook(c).label }}</span>
                            <span class="cap-name">{{ c.label }}</span>
                            @if (c.hint) {
                              <span class="faint">{{ c.hint }}</span>
                            }
                          </li>
                        }
                      </ul>
                    } @else {
                      <p class="faint">The engine did not report what this build supports.</p>
                    }
                  </td>
                </tr>
              }
            }
          </tbody>
        </table>
      }

      @if (e.catalog.length) {
        <h3>Catalog <span class="faint">pinned releases, verified by SHA-256</span></h3>
        @if (installing().length) {
          <ul class="installing" aria-label="Installing">
            @for (d of installing(); track d.id) {
              <li>
                <span class="mono">{{ d.name }}</span>
                <div class="meter" role="progressbar" aria-valuemin="0" aria-valuemax="100" [attr.aria-valuenow]="pct(d.bytesDone, d.bytesTotal)" [attr.aria-label]="'Installing ' + d.name"><span [style.width.%]="pct(d.bytesDone, d.bytesTotal)"></span></div>
                <span class="faint num">{{ size(d.bytesDone) }} / {{ size(d.bytesTotal) }}</span>
              </li>
            }
          </ul>
        }
        <div class="catalog">
          @for (c of e.catalog; track c.id + c.tag) {
            <div class="build" [class.suggested]="c.suggested">
              <div class="btop">
                <span class="bname">{{ c.label }}</span>
                @if (c.suggested) {
                  <span class="pill" data-tone="info"><mat-icon svgIcon="pw:sparkles" aria-hidden="true" />Suggested</span>
                }
              </div>
              <span class="faint num"><span class="mono">{{ c.tag }}</span> · {{ size(c.downloadBytes) }} download</span>
              @if (c.installed) {
                <span class="pill" data-tone="ok"><mat-icon svgIcon="pw:check" aria-hidden="true" />Installed</span>
              } @else {
                <button type="button" class="btn sm" [class.primary]="c.suggested" [disabled]="store.isBusy()('install:' + c.id)" [attr.aria-label]="'Install ' + c.label + ' ' + c.tag" (click)="store.install(c)">
                  <mat-icon svgIcon="pw:download" aria-hidden="true" />Install
                </button>
              }
            </div>
          }
        </div>
      }

      <h3>Processes</h3>
      @if (e.processes.length === 0) {
        <p class="faint none">No process running. One starts when a model loads.</p>
      } @else {
        <table class="table">
          <thead>
            <tr>
              <th scope="col">Model</th>
              <th scope="col">PID</th>
              <th scope="col">Health</th>
              <th scope="col" class="right">Restarts</th>
              <th scope="col" class="right">Memory</th>
              <th scope="col">Started</th>
            </tr>
          </thead>
          <tbody>
            @for (p of e.processes; track p.pid) {
              <tr>
                <td class="bname">{{ model(p.modelId) }}</td>
                <td class="mono faint">{{ p.pid }}</td>
                <td><span class="pill dot" [attr.data-tone]="health(p.health).tone">{{ health(p.health).label }}</span></td>
                <td class="right num" [class.warn]="p.restarts > 0">{{ p.restarts }}</td>
                <td class="right num">{{ size(p.rssBytes) }}</td>
                <td class="faint">{{ when(p.startedAt, now()) }}</td>
              </tr>
            }
          </tbody>
        </table>
      }
    </section>
  `,
  styles: `
    .card {
      padding: var(--sp-5);
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-radius: var(--r-lg);
      box-shadow: var(--shadow-1), var(--card-highlight);
    }
    .head {
      display: flex;
      align-items: flex-start;
      gap: var(--sp-3);
    }
    .logo {
      display: grid;
      place-items: center;
      width: 36px;
      height: 36px;
      border-radius: var(--r-lg);
      background: var(--accent-soft);
      color: var(--accent);
      flex: none;
    }
    .title {
      flex: 1;
      min-width: 0;
      h2 {
        font-size: var(--fs-lg);
      }
      p {
        margin-top: 2px;
        font-size: var(--fs-sm);
      }
    }
    .choice {
      display: flex;
      align-items: flex-end;
      gap: var(--sp-4);
      margin: var(--sp-5) 0 var(--sp-2);
      padding: var(--sp-3) var(--sp-4);
      background: var(--bg-sunken);
      border: 1px solid var(--border-soft);
      border-radius: var(--r-md);
      .field {
        display: grid;
        gap: 4px;
        label {
          font-size: var(--fs-xs);
          font-weight: var(--fw-medium);
          color: var(--text-2);
        }
        .select {
          min-width: 300px;
        }
      }
      .hint {
        flex: 1;
        padding-bottom: 6px;
        font-size: var(--fs-sm);
        color: var(--text-3);
      }
    }
    h3 {
      margin: var(--sp-5) 0 var(--sp-2);
      font-size: var(--fs-sm);
      font-weight: var(--fw-semibold);
      color: var(--text-2);
      .faint {
        margin-left: var(--sp-2);
        font-weight: var(--fw-regular);
      }
    }
    .bname {
      font-weight: var(--fw-medium);
      margin-right: var(--sp-2);
    }
    .upd {
      margin-left: var(--sp-2);
      color: var(--accent);
      border-color: color-mix(in srgb, var(--accent) 35%, transparent);
    }
    .devices {
      max-width: 260px;
      overflow: hidden;
      text-overflow: ellipsis;
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .del:hover:not(:disabled) {
      color: var(--bad);
    }
    .warn {
      color: var(--warn);
      font-weight: var(--fw-medium);
    }
    .catalog {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
      gap: var(--sp-3);
    }
    .build {
      display: flex;
      flex-direction: column;
      align-items: flex-start;
      gap: var(--sp-2);
      padding: var(--sp-3) var(--sp-4);
      border: 1px solid var(--border);
      border-radius: var(--r-md);
      &.suggested {
        background: linear-gradient(180deg, var(--accent-soft), transparent 80%);
        border-color: color-mix(in srgb, var(--accent) 45%, var(--border));
      }
      .faint {
        font-size: var(--fs-xs);
      }
    }
    .btop {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: var(--sp-2);
      width: 100%;
    }
    .pill mat-icon {
      width: 11px;
      height: 11px;
    }
    .installing {
      display: grid;
      gap: var(--sp-2);
      margin: 0 0 var(--sp-3);
      padding: 0;
      list-style: none;
      li {
        display: grid;
        grid-template-columns: minmax(0, 260px) 1fr auto;
        align-items: center;
        gap: var(--sp-3);
        font-size: var(--fs-sm);
      }
    }
    .none {
      font-size: var(--fs-sm);
    }
    .btn.pin.pinned {
      color: var(--accent);
      border-color: color-mix(in srgb, var(--accent) 55%, var(--border));
      background: var(--accent-soft);
    }
    .pinned-note {
      display: flex;
      align-items: center;
      gap: var(--sp-2);
      margin: var(--sp-2) 0 0;
      font-size: var(--fs-sm);
      color: var(--accent);
      mat-icon {
        width: 14px;
        height: 14px;
      }
    }
    .pin-problem {
      margin: var(--sp-2) 0 0;
      font-size: var(--fs-sm);
      color: var(--bad);
    }
    .caps-row td {
      padding-top: 0;
      background: var(--bg-sunken);
    }
    .caps {
      display: grid;
      gap: var(--sp-2);
      margin: var(--sp-2) 0;
      padding: 0;
      list-style: none;
      li {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--sp-3);
        font-size: var(--fs-sm);
      }
      .cap-name {
        min-width: 220px;
        font-weight: var(--fw-medium);
      }
    }
  `,
})
export class EngineCard {
  readonly engine = input.required<Engine>();
  readonly now = input.required<number>();
  protected readonly store = inject(EnginesStore);
  private readonly live = inject(LiveStore);
  private readonly confirm = inject(Confirm);
  protected readonly size = bytes;
  protected readonly when = ago;
  protected readonly pct = percent;
  protected readonly model = modelLabel;
  protected readonly test = (b: InstalledBuild) => selfTestLook(b.selfTest);
  protected readonly health = healthLook;
  protected readonly look = computed(() => {
    const e = this.engine();
    const unhealthy = e.processes.filter((p) => p.health === 'unhealthy').length;
    return unhealthy ? { label: `${unhealthy} unhealthy`, tone: 'bad' as const } : availabilityLook(e.availability);
  });
  protected readonly activeBuild = computed(() => this.engine().builds.find((b) => b.active) ?? null);
  protected readonly installing = computed(() => this.live.downloads().filter((d) => d.kind === 'engine' && ownsDownload(this.engine().name, d.name) && downloadActive(d.state)));

  protected readonly capLook = capabilityLook;
  /** Pin controls exist for llama.cpp only (pack engines have a single build), and once the engine reported its pins. */
  protected readonly canPin = computed(() => this.engine().kind === 'llama.cpp' && this.store.pins() !== null);
  protected readonly capsOpen = signal<ReadonlySet<string>>(new Set());
  protected readonly problems = computed(() => (this.canPin() ? (this.store.pins()?.problems ?? []) : []));
  protected readonly rollbackTarget = computed(() => {
    const t = this.canPin() ? this.store.pins()?.rollbackTo : null;
    if (!t) return null;
    const b = this.engine().builds.find((x) => x.id === t.buildId);
    return { id: t.buildId, tag: t.tag, label: b?.label ?? t.buildId };
  });
  /** Why roll back is not offered (only when the engine has builds to talk about). */
  protected readonly rollbackBlocked = computed(() => (this.canPin() && this.engine().builds.length > 0 ? (this.store.pins()?.rollbackBlocked ?? null) : null));
  /** `Pinned to Vulkan b11669 for all models`, words rather than colour alone. */
  protected readonly pinnedLabel = computed(() => {
    const id = this.canPin() ? this.store.pins()?.global : null;
    const b = id ? this.engine().builds.find((x) => x.id === id) : null;
    return id ? `Pinned to ${b ? `${b.label} ${b.tag}` : id} for all models. Automatic choice is off until you unpin it.` : null;
  });

  protected isPinned(b: InstalledBuild): boolean {
    return this.store.pins()?.global === b.id;
  }

  protected capsOf(b: InstalledBuild): BuildCapability[] | null {
    return this.store.capabilities()[b.id] ?? null;
  }

  protected toggleCaps(id: string): void {
    const next = new Set(this.capsOpen());
    if (!next.delete(id)) next.add(id);
    this.capsOpen.set(next);
  }

  protected async remove(b: InstalledBuild): Promise<void> {
    const ok = await this.confirm.ask({ title: `Remove ${b.label} ${b.tag}?`, message: `Frees ${bytes(b.sizeBytes)}. You can install it again from the catalog.`, confirm: 'Remove', danger: true });
    if (ok) await this.store.remove(b);
  }

  protected async rollback(): Promise<void> {
    const to = this.rollbackTarget();
    if (!to) return;
    const ok = await this.confirm.ask({
      title: `Roll back to ${to.label} ${to.tag}?`,
      message: `${to.label} ${to.tag} is pinned for all models until you unpin it. Models that are loaded keep running; they use it from their next load.`,
      confirm: 'Roll back',
    });
    if (ok) await this.store.rollback();
  }
}
