import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from '@angular/core';
import { PdSelect, type PdSelectOption } from '@decryp7/paddock-ui/select';
import { SuEmptyState } from '@decryp7/paddock-ui';
import { MatIconModule } from '@angular/material/icon';
import { MatTableModule } from '@angular/material/table';
import { MatTooltipModule } from '@angular/material/tooltip';
import type { BuildCapability, Engine, InstalledBuild } from '../../core/api.types';
import { LiveStore } from '../../core/live.store';
import { ago, bytes, bytesIn, bytesUnit, percent, varies } from '../../shared/format';
import { availabilityLook, capabilityLook, downloadActive, healthLook, selfTestLook } from '../../shared/labels';
import { Confirm } from '../../shared/ui/confirm';
import { ItemsMenu, MoreButton, RowMenu, type MenuItem } from '../../shared/ui/menu';
import { EnginesStore } from './engines.store';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatProgressBar } from '@angular/material/progress-bar';

/** A readable model name from its id, for the process table. */
export function modelLabel(id: string): string {
  return id.replace(/[-.](q\d\w*|f16|bf16|f32)$/i, '');
}

/** Engine downloads are named `<engine> <build>`: the card shows only llama.cpp's. */
export function ownsDownload(engineName: string, downloadName: string): boolean {
  return downloadName.startsWith(`${engineName} `);
}

/**
 * An engine that can serve, laid out on the page (no card around the lot): its build choice, the installed builds, the
 * pinned catalog and its processes, each its own surface.
 */
@Component({
  selector: 'pw-engine-card',
  imports: [MatProgressBar, MatIconButton, MatButton, MatIconModule, MatTableModule, MatTooltipModule, SuEmptyState, ItemsMenu, MoreButton, RowMenu, PdSelect],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    @let e = engine();
    <section class="engine" aria-label="llama.cpp">
      <div class="choice">
        <pd-select class="build" inputId="build-choice" label="Build" [options]="buildOptions()" [value]="e.buildChoice" [disabled]="store.isBusy()('select') || e.builds.length === 0" (valueChange)="store.select($event)" />
        <p class="hint">Automatic picks the fastest installed build that passed its self-test on this hardware.</p>
        <span class="status" [attr.data-tone]="look().tone">{{ look().label }}</span>
        @if (rollbackTarget(); as t) {
          <button type="button" matButton="outlined" [disabled]="store.isBusy()('rollback')" (click)="rollback()"><mat-icon svgIcon="pw:rollback" aria-hidden="true" />Roll back to {{ t.label }} {{ t.tag }}</button>
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
        @if (buildsShare(); as share) {
          <span class="faint">{{ share }}</span>
        }
        @if (hasUpdate()) {
          <button type="button" class="link" (click)="toCatalog()" matTooltip="A newer pinned release is in the catalog">Update available</button>
        }
      </div>
      @if (e.builds.length === 0) {
        <pd-empty-state heading="No build installed" description="Pitwall installs the build suggested for this machine by itself. You can also install another from the catalog below."><mat-icon slot="icon" svgIcon="pw:engines" aria-hidden="true" /></pd-empty-state>
      } @else {
        <div class="surface">
          <pw-items-menu #rowMenu label="Build actions" />
          <table mat-table [dataSource]="buildRows()" [trackBy]="byBuild" [multiTemplateDataRows]="true" [fixedLayout]="true" aria-label="Installed builds">
            <ng-container matColumnDef="build">
              <th mat-header-cell *matHeaderCellDef scope="col">Build</th>
              <td mat-cell *matCellDef="let b" [matTooltip]="b.devices.length ? b.label + ' · ' + b.devices.join(', ') : b.label" matTooltipShowDelay="600">
                <span class="bname">{{ b.label }}</span>
                @if (b.active) {
                  <span class="active"><mat-icon svgIcon="pw:check" aria-hidden="true" />Active</span>
                }
                @if (isPinned(b)) {
                  <mat-icon class="pinned" svgIcon="pw:pin" role="img" aria-label="Pinned for all models" matTooltip="Pinned for all models: automatic choice is off" />
                }
              </td>
            </ng-container>
            <ng-container matColumnDef="release">
              <th mat-header-cell *matHeaderCellDef scope="col">Release</th>
              <td mat-cell *matCellDef="let b"><span class="mono">{{ b.tag }}</span></td>
            </ng-container>
            <ng-container matColumnDef="size">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">Size {{ sizeUnit() }}</th>
              <td mat-cell *matCellDef="let b" class="right num">{{ sizeIn(b.sizeBytes, sizeUnit()) }}</td>
            </ng-container>
            <ng-container matColumnDef="test">
              <th mat-header-cell *matHeaderCellDef scope="col">Self-test</th>
              <td mat-cell *matCellDef="let b"><span class="status" [attr.data-tone]="test(b).tone" [matTooltip]="b.selfTestMessage ?? ''">{{ test(b).label }}</span></td>
            </ng-container>
            <ng-container matColumnDef="devices">
              <th mat-header-cell *matHeaderCellDef scope="col">Devices</th>
              <td mat-cell *matCellDef="let b" class="devices" [matTooltip]="b.devices.join(', ')">{{ b.devices.join(', ') || '—' }}</td>
            </ng-container>
            <ng-container matColumnDef="act">
              <th mat-header-cell *matHeaderCellDef scope="col" class="act"><span class="sr-only">Actions</span></th>
              <td mat-cell *matCellDef="let b" class="act">
                @if (canPin()) {
                  <button
                    type="button"
                    matIconButton class="reveal"
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
                <button type="button" matIconButton class="reveal" [class.on]="capsOpen().has(b.id)" [attr.aria-expanded]="capsOpen().has(b.id)" [attr.aria-controls]="'caps-' + b.id" [attr.aria-label]="'What ' + b.label + ' ' + b.tag + ' supports'" matTooltip="Features" (click)="toggleCaps(b.id)">
                  <mat-icon [svgIcon]="capsOpen().has(b.id) ? 'pw:chevron-down' : 'pw:chevron-right'" aria-hidden="true" />
                </button>
                <pw-more-button [menu]="rowMenu.menu()" [items]="menuItems(b)" [label]="'More actions for ' + b.label + ' ' + b.tag" />
              </td>
            </ng-container>
            <!-- A build's features open in a row of their own under it. -->
            <ng-container matColumnDef="caps">
              <td mat-cell *matCellDef="let b" [id]="'caps-' + b.id" [attr.colspan]="buildColumns().length">
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
            </ng-container>
            <tr mat-header-row *matHeaderRowDef="buildColumns(); sticky: true"></tr>
            <tr mat-row *matRowDef="let b; columns: buildColumns()" tabindex="0" (keydown)="onRowKey(b, $event)" pwRowMenu [matContextMenuTriggerFor]="rowMenu.menu()" [matContextMenuTriggerData]="{ items: menuItems(b) }"></tr>
            <tr mat-row *matRowDef="let b; columns: ['caps']; when: capsShown" class="caps-row"></tr>
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
                <mat-progress-bar mode="determinate" [value]="pct(d.bytesDone, d.bytesTotal)" [attr.aria-label]="'Installing ' + d.name" />
                <span class="faint num">{{ size(d.bytesDone) }} / {{ size(d.bytesTotal) }}</span>
              </li>
            }
          </ul>
        }
        <ul class="catalog surface" aria-label="Catalog">
          @for (c of catalogRows(); track c.id + c.tag) {
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
                <button type="button" matButton="outlined" [disabled]="store.isBusy()('install:' + c.id)" [attr.aria-label]="'Install ' + c.label + ' ' + c.tag" (click)="store.install(c)">
                  <mat-icon svgIcon="pw:download" aria-hidden="true" />Install
                </button>
              }
            </li>
          }
        </ul>
      }

      @if (e.leftovers.length) {
        <div class="section-head"><h3>Left over from an older version</h3></div>
        <ul class="leftovers surface" aria-label="Left over files">
          @for (l of e.leftovers; track l.id) {
            <li>
              <span class="bname">{{ l.label }}</span>
              <span class="faint">Pitwall no longer uses it. Nothing is deleted until you remove it.</span>
              <button type="button" matButton="outlined" [disabled]="store.isBusy()('leftover:' + l.id)" (click)="store.removeLeftover(l)">Remove {{ l.label }} ({{ size(l.sizeBytes) }})</button>
            </li>
          }
        </ul>
      }

      <div class="section-head"><h3>Processes</h3></div>
      @if (e.processes.length === 0) {
        <p class="faint none">No process running. One starts when a model loads.</p>
      } @else {
        <div class="surface">
          <table mat-table [dataSource]="e.processes" [trackBy]="byPid" [fixedLayout]="true" aria-label="Engine processes">
            <ng-container matColumnDef="model">
              <th mat-header-cell *matHeaderCellDef scope="col">Model</th>
              <td mat-cell *matCellDef="let p" [matTooltip]="model(p.modelId)" matTooltipShowDelay="600">
                <span class="bname">{{ model(p.modelId) }}</span>
                @if (p.health !== 'healthy') {
                  <span class="status" [attr.data-tone]="health(p.health).tone">{{ health(p.health).label }}</span>
                }
              </td>
            </ng-container>
            <ng-container matColumnDef="pid">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">PID</th>
              <td mat-cell *matCellDef="let p" class="right num faint">{{ p.pid }}</td>
            </ng-container>
            <ng-container matColumnDef="restarts">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">Restarts</th>
              <td mat-cell *matCellDef="let p" class="right num" [class.warn]="p.restarts > 0">{{ p.restarts }}</td>
            </ng-container>
            <ng-container matColumnDef="memory">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">Memory {{ memoryUnit() }}</th>
              <td mat-cell *matCellDef="let p" class="right num">{{ sizeIn(p.rssBytes, memoryUnit()) }}</td>
            </ng-container>
            <ng-container matColumnDef="started">
              <th mat-header-cell *matHeaderCellDef scope="col" class="right">Started</th>
              <td mat-cell *matCellDef="let p" class="right faint">{{ when(p.startedAt, now()) }}</td>
            </ng-container>
            <tr mat-header-row *matHeaderRowDef="processColumns(); sticky: true"></tr>
            <tr mat-row *matRowDef="let p; columns: processColumns()"></tr>
          </table>
        </div>
      }
    </section>
  `,
  styles: `
    .engine {
      display: grid;
      gap: var(--pd-space-3);
    }
    .mat-column-release {
      width: 112px;
    }
    .mat-column-test {
      width: 132px;
    }
    .mat-column-size {
      width: 80px;
    }
    .mat-column-devices {
      width: 22%;
    }
    .mat-column-act {
      width: 116px;
    }
    /* A narrow list keeps the build's name whole: the devices move to its tooltip. */
    @container (max-width: 760px) {
      .mat-column-devices {
        display: none;
      }
    }
    .mat-column-pid,
    .mat-column-restarts {
      width: 88px;
    }
    .mat-column-memory {
      width: 104px;
    }
    .mat-column-started {
      width: 112px;
    }
    .surface {
      /* Clips the table to the rounded corners without becoming a scroller, so the header can stick to the page. */
      overflow: clip;
      container-type: inline-size;
      background: var(--pd-surface-1);
      border: 1px solid var(--pd-hairline);
      border-radius: var(--pd-radius-card);
      box-shadow: var(--pd-shadow-1);
    }
    .choice {
      display: flex;
      align-items: flex-end;
      flex-wrap: wrap;
      gap: var(--pd-space-4);
      .build {
        width: 300px;
        max-width: 100%;
      }
      .hint {
        flex: 1;
        min-width: 240px;
        padding-bottom: 6px;
        font-size: var(--pd-text-label);
        color: var(--pd-text-muted);
      }
    }
    .section-head {
      display: flex;
      align-items: baseline;
      gap: var(--pd-space-3);
      margin-top: var(--pd-space-3);
      h3 {
        font-size: var(--pd-text-body-lg);
        font-weight: var(--pd-weight-semibold);
      }
      .faint {
        font-size: var(--pd-text-label);
      }
    }
    .link {
      padding: 0;
      font: inherit;
      font-size: var(--pd-text-label);
      font-weight: var(--pd-weight-medium);
      color: var(--pd-accent);
      background: none;
      border: none;
      cursor: default;
      &:hover {
        text-decoration: underline;
      }
    }
    .bname {
      font-weight: var(--pd-weight-semibold);
      margin-right: var(--pd-space-2);
    }
    .active {
      display: inline-flex;
      align-items: center;
      gap: 3px;
      margin-right: var(--pd-space-2);
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      mat-icon {
        width: 13px;
        height: 13px;
        color: var(--pd-success);
      }
    }
    .pinned {
      width: 13px;
      height: 13px;
      vertical-align: -2px;
      color: var(--pd-accent);
    }
    .devices {
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
    }
    .warn {
      color: var(--pd-warning);
      font-weight: var(--pd-weight-medium);
    }
    .leftovers {
      margin: 0;
      padding: 0;
      list-style: none;
      li {
        display: grid;
        grid-template-columns: 160px minmax(0, 1fr) auto;
        align-items: center;
        gap: var(--pd-space-3);
        min-height: 44px;
        padding: 0 var(--pd-space-4);
        border-top: 1px solid var(--pd-hairline);
        &:first-child {
          border-top: none;
        }
      }
    }
    .catalog {
      margin: 0;
      padding: 0;
      list-style: none;
      li {
        display: grid;
        grid-template-columns: minmax(180px, 1fr) minmax(0, 1.4fr) 110px 92px;
        align-items: center;
        gap: var(--pd-space-3);
        min-height: 44px;
        padding: 0 var(--pd-space-4);
        border-top: 1px solid var(--pd-hairline);
        &:first-child {
          border-top: none;
        }
      }
      .faint {
        font-size: var(--pd-text-label);
      }
      .mat-mdc-button-base {
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
      font-size: var(--pd-text-label);
      color: var(--pd-text-muted);
      mat-icon {
        width: 12px;
        height: 12px;
      }
    }
    .installing {
      display: grid;
      gap: var(--pd-space-2);
      margin: 0;
      padding: 0;
      list-style: none;
      li {
        display: grid;
        grid-template-columns: minmax(0, 260px) 1fr auto;
        align-items: center;
        gap: var(--pd-space-3);
        font-size: var(--pd-text-label);
      }
    }
    .none {
      font-size: var(--pd-text-label);
    }
    .callout p {
      margin: 0;
    }
    table .caps-row > td,
    table .caps-row:hover > td {
      height: auto;
      padding-top: 0;
      white-space: normal;
      background: var(--pd-surface-2);
    }
    .caps {
      display: grid;
      gap: var(--pd-space-2);
      margin: var(--pd-space-2) 0;
      padding: 0;
      list-style: none;
      li {
        display: flex;
        flex-wrap: wrap;
        align-items: center;
        gap: var(--pd-space-3);
        font-size: var(--pd-text-label);
      }
      .status {
        min-width: 150px;
      }
      .cap-name {
        min-width: 200px;
        font-weight: var(--pd-weight-medium);
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
  protected readonly buildOptions = computed<PdSelectOption[]>(() => [
    { value: 'auto', label: 'Automatic' + (this.activeBuild() ? ' — uses ' + this.activeBuild()!.label : '') },
    ...this.engine().builds.map((b) => ({ value: b.id, label: `${b.label} · ${b.tag}`, disabled: b.selfTest === 'failed' })),
  ]);
  protected readonly look = computed(() => {
    const e = this.engine();
    const unhealthy = e.processes.filter((p) => p.health === 'unhealthy').length;
    return unhealthy ? { label: `${unhealthy} unhealthy`, tone: 'bad' as const } : availabilityLook(e.availability);
  });
  protected readonly activeBuild = computed(() => this.engine().builds.find((b) => b.active) ?? null);
  protected readonly installing = computed(() => this.live.downloads().filter((d) => d.kind === 'engine' && ownsDownload(this.engine().name, d.name) && downloadActive(d.state)));

  protected readonly capLook = capabilityLook;
  /** Pin controls exist for llama.cpp only (pack engines have a single build), and once the engine reported its pins. */
  protected readonly canPin = computed(() => this.store.pins() !== null);
  /** The catalog with the build suggested for this machine first. */
  protected readonly catalogRows = computed(() => [...this.engine().catalog].sort((a, b) => Number(b.suggested) - Number(a.suggested)));
  protected readonly capsOpen = signal<ReadonlySet<string>>(new Set());
  /** The builds as the table's rows: a new list whenever a features row opens or closes, so the table asks `capsShown` again. */
  protected readonly buildRows = computed(() => {
    this.capsOpen();
    return [...this.engine().builds];
  });
  protected readonly capsShown = (_: number, b: InstalledBuild) => this.capsOpen().has(b.id);
  protected readonly byBuild = (_: number, b: InstalledBuild) => b.id + b.tag;
  protected readonly byPid = (_: number, p: { pid: number }) => p.pid;
  /** Release, self-test and devices only when the builds differ in them (UI review 2 rule 7). */
  protected readonly buildColumns = computed(() => {
    const b = this.engine().builds;
    const when = (col: string, differs: boolean) => (differs ? [col] : []);
    return [
      'build',
      ...when('release', varies(b, (x) => x.tag)),
      'size',
      ...when('test', varies(b, (x) => x.selfTest)),
      ...when('devices', varies(b, (x) => x.devices.join(', '))),
      'act',
    ];
  });
  /** What every installed build shares, said once above the table instead of in a constant column. */
  protected readonly buildsShare = computed(() => {
    const b = this.engine().builds;
    if (b.length < 2) return '';
    const cols = this.buildColumns();
    const first = b[0]!;
    return [
      cols.includes('release') ? '' : `all ${first.tag}`,
      cols.includes('test') ? '' : selfTestLook(first.selfTest).label.toLowerCase(),
      cols.includes('devices') || !first.devices.length ? '' : first.devices.join(', '),
    ]
      .filter(Boolean)
      .join(' · ');
  });
  /** Restarts only when some process restarted. */
  protected readonly processColumns = computed(() => ['model', 'pid', ...(this.engine().processes.some((p) => p.restarts > 0) ? ['restarts'] : []), 'memory', 'started']);
  protected readonly sizeUnit = computed(() => bytesUnit(this.engine().builds.map((b) => b.sizeBytes)));
  protected readonly memoryUnit = computed(() => bytesUnit(this.engine().processes.map((p) => p.rssBytes)));
  protected readonly sizeIn = bytesIn;
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
  protected menuItems(b: InstalledBuild): MenuItem[] {
    const items: MenuItem[] = [{ label: this.capsOpen().has(b.id) ? 'Hide features' : 'Features', run: () => this.toggleCaps(b.id) }];
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
    }
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
