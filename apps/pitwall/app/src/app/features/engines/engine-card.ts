import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from '@angular/core';
import { MatIconModule } from '@angular/material/icon';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { BuildCapability, Engine, InstalledBuild } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ago, bytes, percent } from '../../shared/format';
import { availabilityLook, capabilityLook, downloadActive, healthLook, selfTestLook } from '../../shared/labels';
import { Confirm } from '../../shared/ui/confirm';
import { ContextMenuService, type ContextMenuItem } from '../../shared/ui/context-menu';
import { EmptyState } from '../../shared/ui/empty-state';
import { RowMenu } from '../../shared/ui/row-menu';
import { isMenuKey } from '../../shared/ui/row-nav';
import { EnginesStore } from './engines.store';

/** A readable model name from its id, for the process table. */
export function modelLabel(id: string): string {
  return id.replace(/[-.](q\d\w*|f16|bf16|f32)$/i, '');
}

/** Engine downloads are named `<engine> <build>` (llama.cpp builds and pack engines alike): each card shows only its own. */
export function ownsDownload(engineName: string, downloadName: string): boolean {
  return downloadName.startsWith(`${engineName} `);
}

/**
 * An engine that can serve, laid out on the page (no card around the lot): its build choice, the installed builds, the
 * pinned catalog and its processes, each its own surface.
 */
@Component({
  selector: 'pw-engine-card',
  imports: [MatIconModule, MatTooltipModule, EmptyState, RowMenu],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let e = engine();
    <section class="engine" [attr.aria-labelledby]="'eng-' + e.kind">
      <header class="head">
        <div class="logo" aria-hidden="true"><mat-icon svgIcon="pw:engines" /></div>
        <div class="title">
          <h2 [id]="'eng-' + e.kind">{{ e.name }}</h2>
          <p class="muted" [matTooltip]="e.description" matTooltipShowDelay="600">{{ e.description }}</p>
        </div>
        <span class="status" [attr.data-tone]="look().tone">{{ look().label }}</span>
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
        <p class="callout pinned-note" data-tone="info" role="status"><mat-icon svgIcon="pw:pin" aria-hidden="true" />{{ p }}</p>
      }
      @for (problem of problems(); track problem) {
        <p class="callout pin-problem" data-tone="bad" role="alert"><mat-icon svgIcon="pw:alert" aria-hidden="true" />{{ problem }}</p>
      }

      <div class="section-head">
        <h3>Installed builds</h3>
        @if (hasUpdate()) {
          <button type="button" class="link" (click)="toCatalog()" matTooltip="A newer pinned release is in the catalog">Update available</button>
        }
      </div>
      @if (e.builds.length === 0) {
        <pw-empty-state icon="engines" heading="No build installed" text="Install one from the catalog below; the suggested one suits this machine." />
      } @else {
        <div class="surface">
          <table class="table">
            <thead>
              <tr>
                <th scope="col">Build</th>
                <th scope="col">Release</th>
                <th scope="col" class="right">Size</th>
                <th scope="col">Self-test</th>
                <th scope="col">Devices</th>
                <th scope="col"><span class="sr-only">Actions</span></th>
              </tr>
            </thead>
            <tbody>
              @for (b of e.builds; track b.id + b.tag) {
                <tr tabindex="0" [attr.aria-expanded]="capsOpen().has(b.id)" (keydown)="onRowKey(b, $event)" (contextmenu)="onRowMenu(b, $event)">
                  <td>
                    <span class="bname">{{ b.label }}</span>
                    @if (b.active) {
                      <span class="active"><mat-icon svgIcon="pw:check" aria-hidden="true" />Active</span>
                    }
                    @if (isPinned(b)) {
                      <mat-icon class="pinned" svgIcon="pw:pin" role="img" aria-label="Pinned for all models" matTooltip="Pinned for all models: automatic choice is off" />
                    }
                  </td>
                  <td><span class="mono">{{ b.tag }}</span></td>
                  <td class="right num">{{ size(b.sizeBytes) }}</td>
                  <td><span class="status" [attr.data-tone]="test(b).tone" [matTooltip]="b.selfTestMessage ?? ''">{{ test(b).label }}</span></td>
                  <td class="devices"><span class="dev" [matTooltip]="b.devices.join(', ')">{{ b.devices.join(', ') || '—' }}</span></td>
                  <td class="act">
                    <span class="row-actions">
                      @if (canPin()) {
                        <button
                          type="button"
                          class="btn ghost icon sm reveal"
                          [class.on]="isPinned(b)"
                          [attr.aria-pressed]="isPinned(b)"
                          [attr.aria-label]="(isPinned(b) ? 'Unpin ' : 'Pin ') + b.label + ' ' + b.tag + ' for all models'"
                          [matTooltip]="isPinned(b) ? 'Pinned for all models: automatic choice is off. Click to unpin.' : b.selfTest === 'passed' ? 'Use this build for all models, even when a better one is installed' : 'Only builds that passed their self-test can be pinned'"
                          [disabled]="store.isBusy()('pin') || (!isPinned(b) && b.selfTest !== 'passed')"
                          (click)="store.togglePin(b)"
                        >
                          <mat-icon [svgIcon]="isPinned(b) ? 'pw:pin' : 'pw:unpin'" aria-hidden="true" />
                        </button>
                      }
                      <button type="button" class="btn ghost icon sm reveal" [attr.aria-expanded]="capsOpen().has(b.id)" [attr.aria-controls]="'caps-' + b.id" [attr.aria-label]="'What ' + b.label + ' ' + b.tag + ' supports'" matTooltip="Features" (click)="toggleCaps(b.id)">
                        <mat-icon [svgIcon]="capsOpen().has(b.id) ? 'pw:chevron-down' : 'pw:chevron-right'" aria-hidden="true" />
                      </button>
                      <pw-row-menu [items]="menuItems(b)" [label]="'More actions for ' + b.label + ' ' + b.tag" />
                    </span>
                  </td>
                </tr>
                @if (capsOpen().has(b.id)) {
                  <tr class="caps-row">
                    <td [id]="'caps-' + b.id" colspan="6">
                      @if (capsOf(b); as list) {
                        <ul class="caps" [attr.aria-label]="'Features of ' + b.label + ' ' + b.tag">
                          @for (c of list; track c.capability) {
                            <li>
                              <span class="status" [attr.data-tone]="capLook(c).tone">{{ capLook(c).label }}</span>
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
        </div>
      }

      @if (e.catalog.length) {
        <div class="section-head" id="catalog">
          <h3>Catalog</h3>
          <span class="faint">pinned releases, verified by SHA-256</span>
        </div>
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
        <ul class="catalog surface" aria-label="Catalog">
          @for (c of e.catalog; track c.id + c.tag) {
            <li>
              <span class="bname">{{ c.label }}</span>
              <span class="faint num"><span class="mono">{{ c.tag }}</span> · {{ size(c.downloadBytes) }} download</span>
              <span class="rec">
                @if (c.suggested) {
                  <mat-icon svgIcon="pw:sparkles" aria-hidden="true" />Suggested
                }
              </span>
              @if (c.installed) {
                <span class="status" data-tone="ok">Installed</span>
              } @else {
                <button type="button" class="btn sm" [disabled]="store.isBusy()('install:' + c.id)" [attr.aria-label]="'Install ' + c.label + ' ' + c.tag" (click)="store.install(c)">
                  <mat-icon svgIcon="pw:download" aria-hidden="true" />Install
                </button>
              }
            </li>
          }
        </ul>
      }

      <div class="section-head"><h3>Processes</h3></div>
      @if (e.processes.length === 0) {
        <p class="faint none">No process running. One starts when a model loads.</p>
      } @else {
        <div class="surface">
          <table class="table">
            <thead>
              <tr>
                <th scope="col">Model</th>
                <th scope="col">PID</th>
                <th scope="col" class="right">Restarts</th>
                <th scope="col" class="right">Memory</th>
                <th scope="col">Started</th>
              </tr>
            </thead>
            <tbody>
              @for (p of e.processes; track p.pid) {
                <tr>
                  <td>
                    <span class="bname">{{ model(p.modelId) }}</span>
                    @if (p.health !== 'healthy') {
                      <span class="status" [attr.data-tone]="health(p.health).tone">{{ health(p.health).label }}</span>
                    }
                  </td>
                  <td class="mono faint">{{ p.pid }}</td>
                  <td class="right num" [class.warn]="p.restarts > 0">{{ p.restarts }}</td>
                  <td class="right num">{{ size(p.rssBytes) }}</td>
                  <td class="faint">{{ when(p.startedAt, now()) }}</td>
                </tr>
              }
            </tbody>
          </table>
        </div>
      }
    </section>
  `,
  styles: `
    .engine {
      display: grid;
      gap: var(--sp-3);
    }
    .head {
      display: flex;
      align-items: flex-start;
      gap: var(--sp-3);
    }
    .logo {
      display: grid;
      place-items: center;
      width: 32px;
      height: 32px;
      border-radius: var(--r-md);
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
        overflow: hidden;
        font-size: var(--fs-sm);
        text-overflow: ellipsis;
        white-space: nowrap;
      }
    }
    .surface {
      overflow-x: auto;
      background: var(--bg-card);
      border: 1px solid var(--border);
      border-radius: var(--r-lg);
      box-shadow: var(--shadow-1), var(--card-highlight);
    }
    .choice {
      display: flex;
      align-items: flex-end;
      flex-wrap: wrap;
      gap: var(--sp-4);
      .field {
        display: grid;
        gap: 4px;
        label {
          font-size: var(--fs-sm);
          font-weight: var(--fw-medium);
          color: var(--text-2);
        }
        .select {
          min-width: 300px;
        }
      }
      .hint {
        flex: 1;
        min-width: 240px;
        padding-bottom: 6px;
        font-size: var(--fs-sm);
        color: var(--text-2);
      }
    }
    .section-head {
      display: flex;
      align-items: baseline;
      gap: var(--sp-3);
      margin-top: var(--sp-3);
      h3 {
        font-size: var(--fs-md);
        font-weight: var(--fw-semibold);
      }
      .faint {
        font-size: var(--fs-sm);
      }
    }
    .link {
      padding: 0;
      font: inherit;
      font-size: var(--fs-sm);
      font-weight: var(--fw-medium);
      color: var(--accent);
      background: none;
      border: none;
      cursor: default;
      &:hover {
        text-decoration: underline;
      }
    }
    .table th:first-child,
    .table td:first-child {
      padding-left: var(--sp-4);
    }
    .table th:last-child,
    .table td:last-child {
      padding-right: var(--sp-3);
    }
    .bname {
      font-weight: var(--fw-semibold);
      margin-right: var(--sp-2);
    }
    .active {
      display: inline-flex;
      align-items: center;
      gap: 3px;
      margin-right: var(--sp-2);
      font-size: var(--fs-sm);
      color: var(--text-2);
      mat-icon {
        width: 13px;
        height: 13px;
        color: var(--ok);
      }
    }
    .pinned {
      width: 13px;
      height: 13px;
      vertical-align: -2px;
      color: var(--accent);
    }
    .devices {
      max-width: 260px;
      font-size: var(--fs-sm);
      color: var(--text-2);
    }
    .dev {
      display: block;
      max-width: 240px;
      overflow: hidden;
      text-overflow: ellipsis;
    }
    .table td.act {
      width: 1%;
    }
    .warn {
      color: var(--warn);
      font-weight: var(--fw-medium);
    }
    .catalog {
      margin: 0;
      padding: 0;
      list-style: none;
      li {
        display: grid;
        grid-template-columns: minmax(180px, 1fr) minmax(0, 1.4fr) 110px 92px;
        align-items: center;
        gap: var(--sp-3);
        min-height: 44px;
        padding: 0 var(--sp-4);
        border-top: 1px solid var(--border-soft);
        &:first-child {
          border-top: none;
        }
      }
      .faint {
        font-size: var(--fs-sm);
      }
      .btn {
        justify-self: end;
      }
      .status {
        justify-self: end;
      }
    }
    .rec {
      display: inline-flex;
      align-items: center;
      gap: 4px;
      font-size: var(--fs-sm);
      color: var(--text-2);
      mat-icon {
        width: 12px;
        height: 12px;
      }
    }
    .installing {
      display: grid;
      gap: var(--sp-2);
      margin: 0;
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
    .callout p {
      margin: 0;
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
      .status {
        min-width: 150px;
      }
      .cap-name {
        min-width: 200px;
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
  private readonly menu = inject(ContextMenuService);
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

  /** An installed build is older than a pinned release in the catalog: one note in the section header, not a tag on every row. */
  protected readonly hasUpdate = computed(() => this.engine().builds.some((b) => b.outdated));

  protected toCatalog(): void {
    document.getElementById('catalog')?.scrollIntoView({ block: 'center' });
  }

  /** The one list behind a build's "..." button, right-click and Shift+F10. Remove is only here, and asks first. */
  protected menuItems(b: InstalledBuild): ContextMenuItem[] {
    const items: ContextMenuItem[] = [{ label: this.capsOpen().has(b.id) ? 'Hide features' : 'Features', run: () => this.toggleCaps(b.id) }];
    if (this.canPin()) {
      const pinned = this.isPinned(b);
      items.push({ label: pinned ? 'Unpin for all models' : 'Pin for all models', run: () => void this.store.togglePin(b), disabled: this.store.isBusy()('pin') || (!pinned && b.selfTest !== 'passed') });
    }
    items.push({ label: 'Remove…', run: () => void this.remove(b), danger: true, divider: true, disabled: b.active || this.store.isBusy()('remove:' + b.id + ':' + b.tag) });
    return items;
  }

  protected onRowKey(b: InstalledBuild, e: KeyboardEvent): void {
    if (e.target !== e.currentTarget) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      this.toggleCaps(b.id);
    } else if (isMenuKey(e)) {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      this.menu.open(r.left + 48, r.bottom - 8, this.menuItems(b));
    }
  }

  protected onRowMenu(b: InstalledBuild, e: MouseEvent): void {
    e.preventDefault();
    this.menu.open(e.clientX, e.clientY, this.menuItems(b));
  }

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
