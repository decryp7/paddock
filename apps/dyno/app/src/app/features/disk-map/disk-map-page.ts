import { ChangeDetectionStrategy, ChangeDetectorRef, Component, DestroyRef, ElementRef, OnInit, computed, inject, signal, viewChild } from '@angular/core';
import { MatButton, MatIconButton } from '@angular/material/button';
import { MatCell, MatCellDef, MatColumnDef, MatHeaderCell, MatHeaderCellDef, MatHeaderRow, MatHeaderRowDef, MatFooterCell, MatFooterCellDef, MatFooterRow, MatFooterRowDef, MatNoDataRow, MatRow, MatRowDef, MatTable } from '@angular/material/table';
import { MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger } from '@angular/material/menu';
import { MatProgressBar } from '@angular/material/progress-bar';
import { MatSort, MatSortHeader, type Sort } from '@angular/material/sort';
import { MatTooltip } from '@angular/material/tooltip';
import type { IconName } from '../../shared/icons';
import { SuSegmentedControl, readPaddockTokens, type SuSegment } from '@decryp7/paddock-ui';
import { PdSunburst, branchSlots, slotColour, type PdSunburstPoint } from '@decryp7/paddock-ui/chart';
import type { DiskNode, Volume } from '../../core/api.types';
import { PlatformService } from '../../core/platform';
import { UNAVAILABLE, unreadableShort, unreadableText } from '../../core/measured';
import { AsyncState } from '../../shared/async-state';
import { formatBytes, formatCount, formatAgo, formatDate, plural } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { Notifier } from '../../shared/notifier';
import { RowMenu } from '../../shared/row-menu';
import { SearchField } from '../../shared/search-field';
import { injectNow } from '../../shared/now';
import { DiskMapStore } from '../../state/disk-map.store';
import {
  type LevelRow,
  type SortKey,
  VOLUME_GROUPS,
  chartId,
  findRingNode,
  nodeOfChartId,
  toChartTree,
  failureText,
  filterRows,
  jobCount,
  jobFraction,
  nothingReadable,
  jobRate,
  notSavedText,
  revealLabel,
  savedNote,
  phaseLabel,
  shareText,
  sharePercent,
  sortRows,
  stoppedAt,
  volumeFree,
  volumeGroup,
  volumeMeta,
} from './disk-map-model';

/**
 * Disk map: where the space on a volume or folder went. A scan is a job in the engine that reports as it goes; the page shows
 * one folder level at a time (the list beside the ring, or the table, which carries every fact the ring shows) and drills in
 * and out. Read-only: it never deletes, moves or renames anything.
 */
@Component({
  selector: 'dy-disk-map-page',
  imports: [
    AsyncState, Icon, PdSunburst, MatMenu, MatMenuContent, MatMenuItem, MatMenuTrigger, MatProgressBar, RowMenu, SearchField, SuSegmentedControl, MatButton, MatIconButton, MatTooltip,
    MatTable, MatColumnDef, MatHeaderCellDef, MatHeaderCell, MatCellDef, MatCell, MatHeaderRowDef, MatHeaderRow, MatRowDef, MatRow, MatNoDataRow, MatFooterCell, MatFooterCellDef, MatFooterRow, MatFooterRowDef, MatSort, MatSortHeader,
  ],
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { '(keydown)': 'key($event)' },
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Disk map</h1>
          <p>{{ subtitle() }}</p>
        </div>
        @if ((store.outcome() === 'done' || store.outcome() === 'cancelled') && !noAccess()) {
          <button type="button" matButton="filled" [disabled]="store.starting() || store.refreshing() !== null" (click)="store.rescan()"><dy-icon name="refresh" /> Rescan {{ store.status()?.name }}</button>
        }
      </header>

      <dy-async [loaded]="store.volumesLoaded()" [error]="store.volumesError()" what="volumes" [rows]="4" (retry)="load()">
        <div class="toolbar">
          <button type="button" matButton="outlined" class="picker" [matMenuTriggerFor]="pickerMenu" aria-label="Choose a volume" [disabled]="store.outcome() === 'running'">
            <dy-icon [name]="pickerIcon()" /> <span class="picker-name">{{ store.volume()?.name ?? pickerFallback() }}</span> <span class="chev" aria-hidden="true">▾</span>
          </button>
          <span class="divider" aria-hidden="true"></span>
          <button type="button" matButton="outlined" [disabled]="!canGoUp()" [attr.aria-label]="upLabel()" (click)="store.up()"><span aria-hidden="true">↑</span> Go up</button>
          <nav class="crumbs" aria-label="Folder path">
            <ol>
              @for (c of store.trail(); track $index; let last = $last; let i = $index) {
                <li>
                  @if (last) {
                    <span class="crumb current" aria-current="page">{{ c.name }}</span>
                  } @else {
                    <button type="button" class="crumb link" (click)="store.goTo(i)">{{ c.name }}</button>
                    <span class="sep" aria-hidden="true">›</span>
                  }
                </li>
              }
            </ol>
          </nav>
          <span class="spacer"></span>
          <pd-segmented-control label="View" [options]="viewSegs" [value]="store.view()" (valueChange)="store.setView($event === 'table' ? 'table' : 'chart')" />
        </div>

        @if (store.startError(); as e) {
          <div class="callout bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span><button type="button" matButton="text" (click)="store.dismissStartError()">Dismiss</button></div>
        }

        @if (store.restored() && store.capacity(); as c) {
          <section class="capacity" aria-label="Capacity">
            <p class="cap-line">
              @if (c.used !== null) {
                <strong>{{ bytes(c.used) }}</strong> used of {{ bytes(c.capacity) }}
              } @else {
                <strong>{{ bytes(c.capacity) }}</strong> in total
              }
              @if (c.free !== null) {
                <span class="muted"> · {{ bytes(c.free) }} free</span>
              }
              @if (store.volume() && !store.volume()!.wholeVolume) {
                <span class="muted"> · server volume, shown for context</span>
              }
            </p>
            <div class="bar" role="img" [attr.aria-label]="capacityLabel()">
              <span class="seg-counted" [style.width.%]="c.width.counted"></span>
              <span class="seg-pending" [style.width.%]="c.width.notCounted"></span>
              <span class="seg-hidden" [style.width.%]="c.width.hidden"></span>
              <span class="seg-free" [style.width.%]="c.width.free"></span>
            </div>
            <ul class="legend">
              <li><span class="key counted" aria-hidden="true"></span>Counted {{ bytes(c.counted) }}</li>
              @if (c.notCounted > 0) {
                <li><span class="key pending" aria-hidden="true"></span>Not counted yet {{ bytes(c.notCounted) }}</li>
              }
              @if (c.hidden > 0) {
                <li><span class="key hidden" aria-hidden="true"></span>Hidden space {{ bytes(c.hidden) }}</li>
              }
              @if (c.free !== null) {
                <li><span class="key free" aria-hidden="true"></span>Free {{ bytes(c.free) }}</li>
              }
            </ul>
            @if (c.overCounted) {
              <p class="muted small">Counted more than used: clones are counted once per copy.</p>
            }
          </section>
        }

        @if (store.status(); as s) {
          @if (store.refreshingThis() && store.refreshStatus(); as rs) {
            <section class="job" aria-label="Rescan progress">
              <div class="job-top">
                <div class="job-title"><strong>Rescanning {{ rs.name }}</strong><span class="muted">{{ refreshPhase() }} · the map below stays until this is done</span></div>
                <button type="button" matButton="text" (click)="store.showProgress()">Show progress</button>
                <button type="button" matButton="outlined" (click)="store.cancel()">Cancel rescan</button>
              </div>
              @if (refreshFraction() !== null) {
                <mat-progress-bar mode="determinate" [value]="refreshPercent()" [attr.aria-label]="'Rescan of ' + rs.name" [attr.aria-valuetext]="refreshCount()" />
              } @else if (!reducedMotion) {
                <mat-progress-bar mode="indeterminate" [attr.aria-label]="'Rescan of ' + rs.name" [attr.aria-valuetext]="refreshCount()" />
              }
              <p class="job-count" aria-live="off">{{ refreshCount() }}</p>
            </section>
          }
          @if (store.refreshFailure(); as f) {
            <div class="callout bad" role="alert">
              <dy-icon name="alert" />
              <span><strong>The rescan stopped.</strong> The map below is the earlier one and is unchanged.</span>
              <button type="button" matButton="outlined" [disabled]="store.starting()" (click)="store.rescan()">Rescan again</button>
            </div>
          }
          @if (savedLine(); as line) {
            <div class="callout info" role="status" data-testid="saved-map">
              <dy-icon name="info" />
              <span>{{ line }}</span>
              <button type="button" matButton="outlined" (click)="store.forgetSaved()">Forget this map</button>
            </div>
          }
          @if (notSavedLine(); as line) {
            <div class="callout info" role="status" data-testid="not-saved"><dy-icon name="info" /><span>{{ line }}</span></div>
          }
          @if (store.outcome() === 'running') {
            <section class="job" aria-label="Scan progress">
              <div class="job-top">
                <div class="job-title"><strong>Scanning {{ s.name }}</strong><span class="muted">{{ phase() }}</span></div>
                <button type="button" matButton="outlined" (click)="store.cancel()">Cancel scan</button>
              </div>
              @if (fraction() !== null) {
                <mat-progress-bar mode="determinate" [value]="percent()" [attr.aria-label]="'Scan of ' + s.name" [attr.aria-valuetext]="count()" />
              } @else if (!reducedMotion) {
                <mat-progress-bar mode="indeterminate" [attr.aria-label]="'Scan of ' + s.name" [attr.aria-valuetext]="count()" />
              }
              <p class="job-count" aria-live="off">{{ count() }}@if (rate(); as r) { · {{ r }} }</p>
            </section>
          } @else if (store.outcome() === 'cancelled') {
            <div class="callout info" role="status"><dy-icon name="info" /><span><strong>Partial result.</strong> {{ stopped() }}. Folders it did not finish are marked incomplete; Rescan reads everything again.</span></div>
          } @else if (store.outcome() === 'failed' && failure(); as f) {
            <div class="callout bad" role="alert" [attr.data-kind]="f.kind">
              <dy-icon name="alert" />
              <span><strong>{{ f.title }}</strong> {{ f.detail }}</span>
              <button type="button" matButton="outlined" [disabled]="store.starting()" (click)="store.rescan()">Retry scan</button>
            </div>
          }
          @if (s.progress.unreadable > 0 && !noAccess()) {
            <div class="callout warn" role="status">
              <dy-icon name="lock" />
              <span>
                <strong>{{ plural(s.progress.unreadable, 'folder') }} could not be read.</strong> Their sizes are not in the totals.
                @if (s.unreadableFirst.length > 0) {
                  <span class="paths">{{ s.unreadableFirst.join(', ') }}</span>
                }
              </span>
              @if (s.needsFullDiskAccess) {
                <button type="button" matButton="outlined" (click)="store.openFullDiskAccessSettings()">Open Full Disk Access settings</button>
              }
              @if (store.outcome() !== 'running') {
                <button type="button" matButton="outlined" [disabled]="store.starting()" (click)="store.readAgain()">Read again</button>
              }
            </div>
          }
          @if (s.truncated) {
            <div class="callout info" role="status"><dy-icon name="info" /><span>A scan limit left some folders out, so sizes may be larger than shown.</span></div>
          }
        }

        @if (!store.restored()) {
          <div class="loading" aria-busy="true" aria-label="Loading the last scan">
            <div class="skeleton cap"></div>
            <div class="main">
              <section class="card centre"><div class="skeleton ring"></div></section>
              <section class="card list">
                <ul class="rows">
                  @for (n of skeletonRows; track n) {
                    <li class="item"><span class="skeleton swatch-sk"></span><span class="skeleton line"></span></li>
                  }
                </ul>
              </section>
            </div>
          </div>
        } @else if (!store.status()) {
          <section class="card state" aria-label="Not scanned">
            <dy-icon name="disk" />
            <h2>See what fills {{ store.volume()?.name ?? 'a volume' }}</h2>
            <p>Dyno reads the folders once, in the background, and shows where the space went. It never changes or deletes anything.</p>
            @if (store.volume(); as v) {
              <button type="button" matButton="filled" [disabled]="store.starting() || !v.reachable" (click)="store.scan({ volume: { id: v.id } })">Scan {{ v.name }}</button>
              @if (!v.reachable) {
                <p class="muted small">{{ v.name }} is not answering. Try again when the server is back.</p>
              }
            }
            <button type="button" matButton="text" (click)="store.chooseFolder()">Choose a folder…</button>
          </section>
        } @else if (noAccess()) {
          <section class="card state" aria-label="Nothing readable">
            <dy-icon name="lock" class="bad" />
            <h2>Dyno can't read {{ store.status()!.name }}</h2>
            <p>macOS keeps most folders private until you allow it. Without Full Disk Access Dyno can count almost nothing.</p>
            <ol class="steps">
              <li>Open System Settings › Privacy &amp; Security › Full Disk Access.</li>
              <li>Turn on Dyno.</li>
              <li>Come back here and retry the scan.</li>
            </ol>
            <div class="acts">
              <button type="button" matButton="filled" (click)="store.openFullDiskAccessSettings()">Open Full Disk Access settings</button>
              <button type="button" matButton="outlined" [disabled]="store.starting()" (click)="store.readAgain()">Retry scan</button>
            </div>
          </section>
        } @else if (store.levelError(); as e) {
          <div class="callout bad" role="alert"><dy-icon name="alert" /><span>{{ e.message }}</span><button type="button" matButton="outlined" (click)="store.refresh()">Retry</button></div>
        } @else if (!store.level()) {
          <section class="card state" aria-label="Loading" aria-busy="true"><div class="skeleton row"></div><div class="skeleton row"></div><div class="skeleton row"></div></section>
        } @else if (store.level(); as level) {
          @if (level.children.length === 0 && level.other.count === 0 && level.loose.bytes === 0 && store.outcome() !== 'running') {
            <section class="card state" aria-label="Empty folder">
              <dy-icon name="folder" />
              <h2>This folder is empty</h2>
              <p>{{ store.centre()?.name }} has nothing in it that Dyno could count.</p>
            </section>
          } @else if (store.view() === 'chart') {
            <div class="main">
              <section class="card centre" aria-label="This folder">
                <div class="chart-slot">
                  @if (chartRoot() || store.ringsError()) {
                    <pd-sunburst
                      [label]="'Disk usage of ' + store.status()!.name"
                      [root]="chartRoot()"
                      [state]="store.ringsError() ? 'error' : 'ready'"
                      [errorText]="ringsProblem()"
                      (retry)="store.refresh()"
                      [centre]="chartCentre()"
                      [format]="bytes"
                      [rings]="4"
                      [highlight]="rowHover()"
                      [live]="store.outcome() === 'running'"
                      (centreChange)="onCentre($event)"
                      (loadChildren)="onLoad($event)"
                      (openOther)="onOther($event)"
                      (hoverChange)="onHover($event)"
                      (contextMenu)="onChartMenu($event)"
                    />
                  }
                </div>
                <p class="muted">{{ plural(level.node.items, 'item') }}@if (level.node.newestChange) { · newest change {{ date(level.node.newestChange) }} }</p>
                @if (!level.node.complete) {
                  <p class="tag warn"><dy-icon name="alert" /> Incomplete: part of this folder was not read</p>
                }
                <div class="acts">
                  <button type="button" matButton="outlined" (click)="store.reveal(level.node.id)"><dy-icon name="folder" /> {{ revealText }}</button>
                  <button type="button" matButton="outlined" (click)="copyPath(level.node.id)"><dy-icon name="copy" /> Copy path</button>
                </div>
              </section>
              <section class="card list" aria-labelledby="in-folder">
                <div class="card-head"><h2 id="in-folder">In this folder ({{ formatCount(folderCount(level)) }})</h2></div>
                <ul class="rows">
                  @for (r of store.rows(); track r.node.id) {
                    <li class="item" [class.linked]="hoveredId() === chartIdOf(r.node.id)" dyRowMenu [matContextMenuTriggerFor]="rowMenu" [matContextMenuTriggerData]="{ row: r }" (mouseenter)="rowHover.set(chartIdOf(r.node.id))" (mouseleave)="rowHover.set(null)">
                      <span class="swatch" [style.background]="colour(r.rank)" aria-hidden="true"></span>
                      @if (r.drillable) {
                        <button type="button" class="name link" [attr.aria-label]="'Open ' + r.node.name + ', ' + bytes(r.node.bytes) + ', ' + r.share + ' of this folder'" (click)="store.open(r.node)"><dy-icon name="folder" /><span class="txt">{{ r.node.name }}</span></button>
                      } @else {
                        <span class="name"><dy-icon [name]="iconOf(r.node)" /><span class="txt">{{ r.node.name }}</span></span>
                      }
                      @if (!r.node.complete) {
                        <span class="tag warn">{{ tagText(r.node) }}</span>
                      }
                      @if (r.node.kind === 'unreadable') {
                        <span class="size num muted" [matTooltip]="unavailableWhy(r.node.unreadableReason)">{{ unavailable }}</span>
                        <span class="pct num muted" aria-hidden="true">–</span>
                      } @else if (r.node.kind === 'mountPoint') {
                        <span class="size num muted" matTooltip="Another volume: not counted here">–</span>
                        <span class="pct num muted" aria-hidden="true">–</span>
                      } @else {
                        <span class="size num">{{ bytes(r.node.bytes) }}</span>
                        <span class="pct num muted">{{ r.share }}</span>
                      }
                      <span class="acts-row"><button type="button" matIconButton [matTooltip]="revealText" [attr.aria-label]="revealText + ': ' + r.node.name" (click)="store.reveal(r.node.id)"><dy-icon name="folder" /></button><button type="button" matIconButton matTooltip="Copy path" [attr.aria-label]="'Copy path of ' + r.node.name" (click)="copyPath(r.node.id)"><dy-icon name="copy" /></button></span>
                    </li>
                  }
                  @if (level.other.count > 0 && level.other.bytes > 0) {
                    <li class="item other">
                      <span class="swatch" aria-hidden="true"></span>
                      <span class="name"><dy-icon name="layers" /><span class="txt">{{ plural(level.other.count, 'smaller item') }}</span></span>
                      <span class="size num">{{ bytes(level.other.bytes) }}</span>
                      <span class="pct num muted">{{ share(level.other.bytes, level.node.bytes) }}</span>
                      <span class="acts-row"><button type="button" matButton="text" (click)="store.setView('table')">Show in table</button></span>
                    </li>
                  }
                  @if (level.loose.bytes > 0) {
                    <li class="item other">
                      <span class="swatch" aria-hidden="true"></span>
                      <span class="name"><dy-icon name="file" /><span class="txt">Files directly in this folder</span></span>
                      <span class="size num">{{ bytes(level.loose.bytes) }}</span>
                      <span class="pct num muted">{{ share(level.loose.bytes, level.node.bytes) }}</span>
                      <span class="acts-row"></span>
                    </li>
                  }
                </ul>
                @if (level.nextOffset !== null) {
                  <div class="more"><button type="button" matButton="outlined" (click)="store.loadMore()">Show more</button></div>
                }
              </section>
            </div>
          } @else {
            <section class="card table-card" aria-label="Folder contents">
              <div class="table-tools">
                <dy-search-field label="Filter by name" placeholder="Filter by name" [value]="filter()" (valueChange)="filter.set($event)" />
                <span class="muted small">{{ tableRows().length + (loose() ? 1 : 0) }} of {{ formatCount(level.shown + (loose() ? 1 : 0)) }} rows</span>
              </div>
              @if (onlyIds(); as only) {
                <p class="only muted small" role="status">Showing {{ plural(only.size, 'smaller item') }} from the chart. <button type="button" matButton="text" (click)="onlyIds.set(null)">Show all</button></p>
              }
              <div class="tablewrap">
                <table mat-table matSort matSortDisableClear [dataSource]="tableRows()" [matSortActive]="sort().key" [matSortDirection]="sort().dir" (matSortChange)="sortChange($event)" [trackBy]="trackRow" aria-label="Contents of {{ level.node.name }}">
                  <ng-container matColumnDef="name">
                    <th mat-header-cell *matHeaderCellDef mat-sort-header="name">Name</th>
                    <td mat-cell *matCellDef="let r" class="nm">
                      <span class="swatch" [style.background]="colour(r.rank)" aria-hidden="true"></span>
                      @if (r.drillable) {
                        <button type="button" matButton="text" class="name" (click)="store.open(r.node)"><dy-icon name="folder" />{{ r.node.name }}</button>
                      } @else {
                        <span class="name"><dy-icon [name]="iconOf(r.node)" />{{ r.node.name }}</span>
                      }
                      @if (!r.node.complete) {
                        <span class="tag warn">{{ tagText(r.node) }}</span>
                      }
                    </td>
                    <td mat-footer-cell *matFooterCellDef class="nm">@if (loose(); as l) { <span class="swatch" aria-hidden="true"></span><span class="name"><dy-icon name="file" /><span class="txt">Files directly in this folder</span></span> }</td>
                  </ng-container>
                  <ng-container matColumnDef="bytes">
                    <th mat-header-cell *matHeaderCellDef mat-sort-header="bytes" class="num">Size</th>
                    <td mat-cell *matCellDef="let r" class="num">
                      @if (r.node.kind === 'unreadable') {
                        <span class="muted" [matTooltip]="unavailableWhy(r.node.unreadableReason)">{{ unavailable }}</span>
                      } @else if (r.node.kind === 'mountPoint') {
                        <span class="muted" matTooltip="Another volume: not counted here">–</span>
                      } @else {
                        {{ bytes(r.node.bytes) }}
                      }
                    </td>
                    <td mat-footer-cell *matFooterCellDef class="num">@if (loose(); as l) { {{ bytes(l.bytes) }} }</td>
                  </ng-container>
                  <ng-container matColumnDef="items">
                    <th mat-header-cell *matHeaderCellDef mat-sort-header="items" class="num">Items</th>
                    <td mat-cell *matCellDef="let r" class="num">{{ isCounted(r.node) ? formatCount(r.node.items) : '–' }}</td>
                    <td mat-footer-cell *matFooterCellDef class="num">@if (loose(); as l) { {{ formatCount(l.items) }} }</td>
                  </ng-container>
                  <ng-container matColumnDef="newestChange">
                    <th mat-header-cell *matHeaderCellDef mat-sort-header="newestChange" class="num">Newest change</th>
                    <td mat-cell *matCellDef="let r" class="num muted">{{ date(r.node.newestChange) }}</td>
                    <td mat-footer-cell *matFooterCellDef></td>
                  </ng-container>
                  <ng-container matColumnDef="share">
                    <th mat-header-cell *matHeaderCellDef class="share-col">Share</th>
                    <td mat-cell *matCellDef="let r" class="share-col">
                      @if (!isCounted(r.node)) {
                        <span class="muted" aria-hidden="true">–</span>
                      } @else {
                        <span class="minibar" aria-hidden="true"><span [style.width.%]="r.percent"></span></span><span class="num">{{ r.share }}</span>
                      }
                    </td>
                    <td mat-footer-cell *matFooterCellDef class="share-col">@if (loose(); as l) { <span class="minibar" aria-hidden="true"><span [style.width.%]="l.percent"></span></span><span class="num">{{ l.share }}</span> }</td>
                  </ng-container>
                  <ng-container matColumnDef="actions">
                    <th mat-header-cell *matHeaderCellDef class="row-acts"><span class="visually-hidden">Actions</span></th>
                    <td mat-cell *matCellDef="let r" class="row-acts"><button type="button" matIconButton [matTooltip]="revealText" [attr.aria-label]="revealText + ': ' + r.node.name" (click)="store.reveal(r.node.id)"><dy-icon name="folder" /></button><button type="button" matIconButton matTooltip="Copy path" [attr.aria-label]="'Copy path of ' + r.node.name" (click)="copyPath(r.node.id)"><dy-icon name="copy" /></button></td>
                    <td mat-footer-cell *matFooterCellDef class="row-acts"></td>
                  </ng-container>
                  <tr mat-header-row *matHeaderRowDef="tableColumns"></tr>
                  <tr mat-row *matRowDef="let r; columns: tableColumns" dyRowMenu [matContextMenuTriggerFor]="rowMenu" [matContextMenuTriggerData]="{ row: r }"></tr>
                  <tr mat-footer-row *matFooterRowDef="tableColumns" [class.foot-off]="!loose()"></tr>
                  <tr class="mat-row" *matNoDataRow><td class="muted none" [attr.colspan]="tableColumns.length">No row matches "{{ filter() }}".</td></tr>
                </table>
              </div>
              <div class="table-foot">
                <span class="muted small">{{ upKey }} goes up a folder. Right-click a row for more.</span>
                @if (level.nextOffset !== null) {
                  <button type="button" matButton="outlined" (click)="store.loadMore()">Show more rows</button>
                }
              </div>
            </section>
          }
        }
      </dy-async>
    </div>

    <mat-menu #pickerMenu="matMenu" class="vol-menu" aria-label="Volumes">
      @for (g of groups; track g.id) {
        @if (volumesOf(g.id).length > 0) {
          <div class="menu-label" role="presentation">{{ g.label }}@if (g.hint) { <span class="muted"> · {{ g.hint }}</span> }</div>
          @for (v of volumesOf(g.id); track v.id) {
            <button type="button" mat-menu-item (click)="store.selectVolume(v.id)">
              <dy-icon [name]="g.id === 'network' ? 'network' : g.id === 'external' ? 'external' : 'disk'" />
              <span class="vol">
                <strong>{{ v.name }}</strong>
                <span class="muted">{{ meta(v) }} · @if (v.reachable) { {{ free(v) }} } @else { <span class="warn-text"><dy-icon name="alert" /> Not answering</span> }</span>
              </span>
            </button>
          }
        }
      }
      <button type="button" mat-menu-item (click)="store.chooseFolder()"><dy-icon name="folder" /> Choose a folder…</button>
    </mat-menu>

    <mat-menu #rowMenu="matMenu" aria-label="Folder actions">
      <ng-template matMenuContent let-row="row">
        @if (row.drillable) {
          <button type="button" mat-menu-item (click)="store.open(row.node)"><dy-icon name="folder" /> Open</button>
        }
        <button type="button" mat-menu-item (click)="store.reveal(row.node.id)"><dy-icon name="folder" /> {{ revealText }}</button>
        <button type="button" mat-menu-item (click)="copyPath(row.node.id)"><dy-icon name="copy" /> Copy path</button>
        <button type="button" mat-menu-item (click)="copySize(row.node)"><dy-icon name="copy" /> Copy size</button>
      </ng-template>
    </mat-menu>

    <span #chartMenuAnchor class="menu-anchor" aria-hidden="true" [matMenuTriggerFor]="rowMenu" (menuClosed)="refocusChart()" [style.left.px]="menuAt().x" [style.top.px]="menuAt().y"></span>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-4); align-content: start; max-width: 1440px; }
    .page-head { margin-bottom: 0; }
    dy-async { display: grid; gap: var(--sp-4); }
    .toolbar { display: flex; align-items: center; gap: var(--sp-2); min-height: 44px; flex-wrap: wrap; }
    .toolbar .spacer { flex: 1; }
    .divider { width: 1px; height: 20px; background: var(--border); }
    .picker-name { max-width: 220px; overflow: hidden; text-overflow: ellipsis; }
    .chev { color: var(--text-3); font-size: var(--fs-xs); }
    .crumbs ol { display: flex; align-items: center; gap: var(--sp-1); margin: 0; padding: 0; list-style: none; flex-wrap: wrap; }
    .crumbs li { display: inline-flex; align-items: center; gap: var(--sp-1); }
    .crumb { font-size: var(--fs-base); }
    .crumb.current { font-weight: var(--fw-semibold); color: var(--text-1); }
    .link { border: 0; background: none; padding: 0; color: var(--text-2); font: inherit; cursor: pointer; }
    .link:hover { color: var(--text-1); text-decoration: underline; }
    .link:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; border-radius: var(--r-sm); }
    .sep { color: var(--text-3); }
    .muted { color: var(--text-3); }
    .small { font-size: var(--fs-sm); }
    .callout { display: flex; gap: var(--sp-3); align-items: center; padding: var(--sp-3) var(--sp-4); border: 1px solid var(--border); border-radius: var(--r-md); font-size: var(--fs-sm); color: var(--text-1); }
    .callout > span { flex: 1; }
    .callout.bad { background: var(--bad-soft); border-color: color-mix(in srgb, var(--bad) 35%, transparent); }
    .callout.bad > dy-icon { color: var(--bad); }
    .callout.warn { background: var(--warn-soft); border-color: color-mix(in srgb, var(--warn) 35%, transparent); }
    .callout.warn > dy-icon { color: var(--warn); }
    .callout.info { background: var(--info-soft); border-color: color-mix(in srgb, var(--info) 30%, transparent); }
    .callout.info > dy-icon { color: var(--info); }
    .paths { display: block; margin-top: 2px; color: var(--text-2); overflow-wrap: anywhere; }
    .capacity { display: grid; gap: var(--sp-2); }
    .cap-line { margin: 0; font-size: var(--fs-base); }
    .bar { display: flex; height: 8px; border-radius: 4px; overflow: hidden; background: var(--surface-inset); border: 1px solid var(--border); }
    .bar span { display: block; height: 100%; }
    .seg-counted { background: var(--chart-8); }
    .seg-pending { background: repeating-linear-gradient(135deg, var(--border-strong) 0 2px, transparent 2px 5px); }
    .seg-hidden { background: repeating-linear-gradient(45deg, var(--text-3) 0 2px, var(--surface-3) 2px 5px); }
    .seg-free { background: transparent; }
    .legend { display: flex; gap: var(--sp-4); margin: 0; padding: 0; list-style: none; flex-wrap: wrap; color: var(--text-2); font-size: var(--fs-sm); }
    .legend li { display: inline-flex; align-items: center; gap: var(--sp-2); }
    .key { display: inline-block; width: 12px; height: 8px; border-radius: 2px; border: 1px solid var(--border-strong); }
    .key.counted { background: var(--chart-8); }
    .key.pending { background: repeating-linear-gradient(135deg, var(--border-strong) 0 2px, transparent 2px 5px); }
    .key.hidden { background: repeating-linear-gradient(45deg, var(--text-3) 0 2px, var(--surface-3) 2px 5px); }
    .key.free { background: var(--surface-inset); }
    .job { display: grid; gap: var(--sp-2); padding: var(--sp-3) var(--sp-4); border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); }
    .job-top { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-3); }
    .job-title { display: grid; gap: 2px; }
    .job-count { margin: 0; color: var(--text-2); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
    .state { display: grid; justify-items: center; gap: var(--sp-3); padding: var(--sp-10) var(--sp-4); text-align: center; }
    .state dy-icon { --icon-size: 32px; color: var(--text-3); }
    .state dy-icon.bad { color: var(--bad); }
    .state h2 { margin: 0; font-size: var(--fs-lg); }
    .state p { margin: 0; max-width: 52ch; color: var(--text-2); }
    .steps { margin: 0; padding-left: var(--sp-5); text-align: left; color: var(--text-2); display: grid; gap: var(--sp-1); }
    .loading { display: grid; gap: var(--sp-4); }
    .skeleton.cap { height: 40px; max-width: 100%; }
    .skeleton.ring { width: 280px; height: 280px; border-radius: 50%; margin: var(--sp-4) auto; }
    .skeleton.swatch-sk { width: 12px; height: 12px; }
    .skeleton.line { grid-column: 2 / -1; height: 12px; }
    .skeleton.row { width: 100%; max-width: 640px; height: 14px; }
    .main { display: grid; grid-template-columns: minmax(300px, 616px) minmax(0, 1fr); gap: var(--sp-4); align-items: start; }
    @media (max-width: 1100px) { .main { grid-template-columns: minmax(0, 1fr); } }
    .centre { display: grid; gap: var(--sp-2); padding: var(--sp-5); justify-items: start; }
    .centre p { margin: 0; }
    .chart-slot { width: 100%; max-width: 560px; margin: 0 auto; }
    .chart-slot:empty { display: none; }
    .item.linked { background: var(--surface-3); }
    .only { margin: 0; padding: 0 var(--sp-4) var(--sp-2); }
    .acts { display: flex; gap: var(--sp-2); margin-top: var(--sp-2); }
    .list { padding-bottom: var(--sp-2); }
    .rows { margin: 0; padding: var(--sp-2) 0 0; list-style: none; }
    .item { display: grid; grid-template-columns: 12px minmax(0, 1fr) auto 56px 56px; gap: var(--sp-3); align-items: center; min-height: 40px; padding: 0 var(--sp-4); border-top: 1px solid var(--border-subtle); }
    .item:hover, .item:focus-within { background: var(--surface-3); }
    .item .name { display: inline-flex; align-items: center; gap: var(--sp-2); min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-1); text-align: left; }
    .item .name dy-icon, td .name dy-icon { --icon-size: 15px; color: var(--text-3); flex: none; }
    .item .name .txt { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    .item .name:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
    .item .tag { grid-column: 2; justify-self: start; }
    .num { font-variant-numeric: tabular-nums; text-align: right; }
    .swatch { width: 12px; height: 12px; border-radius: 3px; background: var(--chart-8); border: 1px solid var(--border-strong); }
    .tag { display: inline-flex; align-items: center; gap: var(--sp-1); font-size: var(--fs-xs); font-weight: var(--fw-medium); }
    .tag.warn { color: var(--warn); }
    .tag dy-icon { --icon-size: 13px; }
    .more { padding: var(--sp-3) var(--sp-4); }
    .table-card { overflow: hidden; }
    .table-tools { display: flex; align-items: center; justify-content: space-between; gap: var(--sp-3); padding: var(--sp-3) var(--sp-4); }
    .tablewrap { overflow: auto; }
    .share-col { white-space: nowrap; }
    table { width: 100%; table-layout: fixed; }
    .mat-column-bytes { width: 76px; }
    .mat-column-items { width: 64px; }
    /* Wide enough for "Newest change" and its sort arrow on one line. */
    .mat-column-newestChange { width: 132px; }
    th.mat-column-newestChange, th.mat-column-newestChange ::ng-deep .mat-sort-header-content { white-space: nowrap; text-align: right; }
    .mat-column-share { width: 100px; }
    .mat-column-actions { width: 64px; }
    th, td { white-space: nowrap; }
    th.mat-mdc-header-cell, td.mat-mdc-cell, td.mat-mdc-footer-cell { padding-inline: var(--sp-2); }
    td.mat-mdc-footer-cell { font: inherit; color: var(--text-1); }
    td.num, td.share-col { overflow: visible; text-overflow: clip; }
    td.nm { overflow: hidden; text-overflow: ellipsis; }
    td.nm button.name { padding: 0 var(--sp-1); min-width: 0; max-width: calc(100% - 28px); vertical-align: middle; }
    td.nm .txt { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
    td.nm .name { min-width: 0; display: inline-flex; align-items: center; gap: var(--sp-2); vertical-align: middle; }
    td.nm .swatch { display: inline-block; margin-right: var(--sp-2); vertical-align: middle; }
    td.nm .tag { margin-left: var(--sp-2); }
    th.num, td.num { text-align: right; }
    td.row-acts { white-space: nowrap; text-align: right; overflow: visible; text-overflow: clip; padding-inline: 0 var(--sp-2); }
    td.row-acts button { opacity: 0; --mat-icon-button-state-layer-size: 32px; padding: 4px; }
    tr:hover td.row-acts button, tr:focus-within td.row-acts button, td.row-acts button:focus-visible { opacity: 1; }
    .acts-row { opacity: 0; display: flex; gap: 2px; justify-content: flex-end; }
    .item:hover .acts-row, .item:focus-within .acts-row { opacity: 1; }
    .minibar { display: inline-block; width: 36px; height: 6px; margin-right: var(--sp-2); border-radius: 3px; background: var(--surface-inset); vertical-align: middle; overflow: hidden; }
    .minibar span { display: block; height: 100%; background: var(--chart-8); }
    .none { text-align: center; padding: var(--sp-6); }
    .table-foot { display: flex; align-items: center; justify-content: space-between; padding: var(--sp-3) var(--sp-4); border-top: 1px solid var(--border-subtle); }
    tr.foot-off { display: none; }
    .menu-anchor { position: fixed; width: 0; height: 0; pointer-events: none; }
    .menu-label { padding: var(--sp-2) var(--sp-4) var(--sp-1); font-size: var(--fs-xs); font-weight: var(--fw-medium); color: var(--text-3); }
    .vol { display: grid; text-align: left; }
    .vol .muted { font-size: var(--fs-sm); }
    .warn-text { color: var(--warn); }
  `,
})
export class DiskMapPage implements OnInit {
  protected readonly store = inject(DiskMapStore);
  private readonly notifier = inject(Notifier);
  private readonly platform = inject(PlatformService).platform;
  /** The shortcut that goes up a folder, as this system writes it. */
  protected readonly upKey = this.platform === 'macos' ? '\u2325\u2191' : 'Alt+Up';
  private readonly now = injectNow();

  protected readonly skeletonRows = [1, 2, 3, 4, 5, 6, 7, 8, 9];
  protected readonly groups = VOLUME_GROUPS;
  /** The rows a folder's list or table shows: the rows above the fold, the one row for the folded small ones (when it has a size) and the files that have no row of their own. */
  protected folderCount(level: { shown: number; other: { count: number; bytes: number }; loose: { bytes: number } }): number {
    return level.shown + (level.other.count > 0 && level.other.bytes > 0 ? 1 : 0) + (level.loose.bytes > 0 ? 1 : 0);
  }
  protected readonly tableColumns = ['name', 'bytes', 'items', 'newestChange', 'share', 'actions'];
  protected readonly viewSegs: SuSegment[] = [{ value: 'chart', label: 'Chart' }, { value: 'table', label: 'Table' }];
  protected readonly trackRow = (_: number, r: LevelRow): number => r.node.id;
  protected readonly filter = signal('');
  protected readonly sort = signal<{ key: SortKey; dir: 'asc' | 'desc' }>({ key: 'bytes', dir: 'desc' });

  /** The children a click on a folded chart segment stands for (the table then shows only those). */
  protected readonly onlyIds = signal<Set<number> | null>(null);
  protected readonly tableRows = computed<LevelRow[]>(() => {
    const only = this.onlyIds();
    const rows = only ? this.store.rows().filter((r) => only.has(r.node.id)) : this.store.rows();
    return sortRows(filterRows(rows, this.filter()), this.sort().key, this.sort().dir);
  });
  /** The segment under the pointer or focus in the chart, and the row under the pointer: each highlights the other. */
  protected readonly hoveredId = signal<string | null>(null);
  protected readonly rowHover = signal<string | null>(null);
  private readonly cdr = inject(ChangeDetectorRef);
  private readonly chartMenu = viewChild.required('chartMenuAnchor', { read: MatMenuTrigger });
  /** Where the chart's own context menu opens (the pointer, or the focused segment). */
  protected readonly menuAt = signal({ x: 0, y: 0 });
  protected readonly reducedMotion = typeof matchMedia === 'function' && matchMedia('(prefers-reduced-motion: reduce)').matches;
  protected readonly unavailable = UNAVAILABLE;
  protected readonly unavailableWhy = unreadableText;
  /** Bumped when the theme or accent changes: the ring order of the colours depends on the resolved palette. */
  private readonly appearance = signal(0);
  private readonly slots = computed(() => {
    this.appearance();
    return branchSlots(readPaddockTokens(this.hostEl).palette);
  });
  private readonly hostEl = inject<ElementRef<HTMLElement>>(ElementRef).nativeElement;
  protected readonly chartRoot = computed(() => {
    const rings = this.store.rings();
    const c = this.store.capacity();
    const whole = this.store.status()?.volume?.wholeVolume === true;
    return rings ? toChartTree(rings, this.store.ancestors(), whole && c ? { hidden: c.hidden, pending: c.notCounted } : undefined) : null;
  });
  protected readonly ringsProblem = computed(() => `Couldn't draw the rings: ${this.store.ringsError()?.message ?? ''}`);
  protected readonly chartCentre = computed(() => {
    const id = this.store.rings()?.node.id;
    return id === undefined ? null : chartId(id);
  });
  protected readonly chartIdOf = chartId;
  protected readonly canGoUp = computed(() => this.store.trail().length > 1);
  protected readonly upLabel = computed(() => {
    const t = this.store.trail();
    return t.length > 1 ? `Go up to ${t[t.length - 2]!.name}` : 'Go up';
  });
  protected readonly noAccess = computed(() => nothingReadable(this.store.status()));
  protected readonly fraction = computed(() => (this.store.status() ? jobFraction(this.store.status()!.progress) : null));
  protected readonly percent = computed(() => Math.round((this.fraction() ?? 0) * 100));
  protected readonly count = computed(() => (this.store.status() ? jobCount(this.store.status()!.progress) : ''));
  protected readonly phase = computed(() => (this.store.status() ? phaseLabel(this.store.status()!) : ''));
  protected readonly stopped = computed(() => (this.store.status() ? stoppedAt(this.store.status()!.progress) : ''));
  protected readonly failure = computed(() => {
    const s = this.store.status();
    return s && typeof s.state !== 'string' ? failureText(s.state.failed.failure, s.name) : null;
  });
  protected readonly subtitle = computed(() => {
    const s = this.store.status();
    const v = this.store.volume();
    const what = s && !s.volume ? `${s.name} · folder` : v ? `${v.name} · ${volumeMeta(v)}` : 'Where the space on a volume or folder went';
    if (!s) return what;
    const state = { running: 'scanning now', done: 'scanned', cancelled: 'partial scan, stopped by you', failed: 'scan stopped' }[this.store.outcome()!];
    const saved = typeof s.origin === 'string' ? '' : ' (saved)';
    const when = s.scannedAt !== null && this.store.outcome() === 'done' ? `scanned ${formatAgo(s.scannedAt, this.now())}${saved}` : state;
    return `${what} · ${when}`;
  });
  protected readonly pickerIcon = computed(() => {
    const v = this.store.volume();
    const g = v ? volumeGroup(v) : 'internal';
    return g === 'network' ? 'network' : g === 'external' ? 'external' : 'disk';
  });

  protected readonly revealText = revealLabel(this.platform);
  protected readonly rate = computed(() => (this.store.status() ? jobRate(this.store.status()!) : null));
  protected readonly refreshFraction = computed(() => (this.store.refreshStatus() ? jobFraction(this.store.refreshStatus()!.progress) : null));
  protected readonly refreshPercent = computed(() => Math.round((this.refreshFraction() ?? 0) * 100));
  protected readonly refreshCount = computed(() => (this.store.refreshStatus() ? jobCount(this.store.refreshStatus()!.progress) : ''));
  protected readonly refreshPhase = computed(() => (this.store.refreshStatus() ? phaseLabel(this.store.refreshStatus()!) : ''));
  /** The line under a map read back from the cache: its age, the net change in used space, and what Rescan does. */
  protected readonly savedLine = computed(() => {
    const s = this.store.status();
    return s && s.scannedAt !== null ? savedNote(s, formatAgo(s.scannedAt, this.now()), formatBytes) : null;
  });
  protected readonly notSavedLine = computed(() => {
    const s = this.store.status();
    return s ? notSavedText(s) : null;
  });
  protected readonly formatCount = formatCount;
  protected readonly plural = plural;
  protected readonly bytes = formatBytes;
  protected readonly meta = volumeMeta;
  protected readonly free = volumeFree;
  protected readonly share = shareText;

  constructor() {
    const observer = new MutationObserver(() => this.appearance.update((n) => n + 1));
    observer.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme', 'data-accent', 'class'] });
    inject(DestroyRef).onDestroy(() => observer.disconnect());
  }

  /** The list swatch's colour: the ring's, by rank (the five largest, then neutral). */
  protected colour(rank: number): string {
    return slotColour(rank < 5 ? (this.slots()[rank] ?? rank) : -1);
  }

  ngOnInit(): void {
    void this.load();
  }

  protected async load(): Promise<void> {
    await this.store.loadVolumes();
    await this.store.restore();
    await this.store.refreshOnOpen(this.now());
  }

  protected pickerFallback(): string {
    return this.store.volumes().length === 0 ? 'No volumes' : 'Choose a volume';
  }

  protected volumesOf(group: string): Volume[] {
    return this.store.volumes().filter((v) => volumeGroup(v) === group);
  }

  protected date(at: number): string {
    return formatDate(at, this.now());
  }

  protected capacityLabel(): string {
    const c = this.store.capacity();
    if (!c) return '';
    const parts = [`Counted ${formatBytes(c.counted)}`];
    if (c.notCounted > 0) parts.push(`not counted yet ${formatBytes(c.notCounted)}`);
    if (c.hidden > 0) parts.push(`hidden space ${formatBytes(c.hidden)}`);
    if (c.free !== null) parts.push(`free ${formatBytes(c.free)}`);
    return parts.join(', ');
  }

  protected sortChange(e: Sort): void {
    const key = this.tableColumns.includes(e.active) ? (e.active as SortKey) : 'bytes';
    this.sort.set({ key, dir: e.direction === 'asc' ? 'asc' : 'desc' });
  }

  protected onCentre(id: string | null): void {
    const n = id === null ? null : nodeOfChartId(id);
    if (id !== null && n === null) return;
    const trail = this.store.trail();
    const at = n === null || n === this.store.rootId() ? 0 : trail.findIndex((c) => c.node === n);
    if (at >= 0) void this.store.goTo(at);
    else void this.store.openTo(n!);
  }

  protected onLoad(id: string): void {
    const n = nodeOfChartId(id);
    if (n !== null) void this.store.openTo(n);
  }

  /** A folded or "other" segment: its folder becomes the centre and the table shows just those items. */
  protected async onOther(e: { parent: string; ids: string[] }): Promise<void> {
    const parent = nodeOfChartId(e.parent);
    if (parent !== null) await this.store.openTo(parent);
    const ids = e.ids.map(nodeOfChartId).filter((n): n is number => n !== null);
    this.onlyIds.set(ids.length > 0 ? new Set(ids) : null);
    await this.store.setView('table');
  }

  protected onHover(p: PdSunburstPoint | null): void {
    this.hoveredId.set(p?.node.id ?? null);
  }

  /** The chart's right-click or Shift+F10: the same menu as a row's, for the folder or file under it. */
  /** The "Files directly in this folder" row of the table: the files that have no row of their own, so the rows add up. */
  protected readonly loose = computed(() => {
    const level = this.store.level();
    if (!level || level.loose.bytes === 0 || this.filter().trim() !== '' || this.onlyIds() !== null) return null;
    return { bytes: level.loose.bytes, items: level.loose.items, percent: sharePercent(level.loose.bytes, level.node.bytes), share: shareText(level.loose.bytes, level.node.bytes) };
  });

  /** Mount points and refused folders have no size of their own here: the list and the table show "–" or Unavailable. */
  protected isCounted(node: DiskNode): boolean {
    return node.kind !== 'unreadable' && node.kind !== 'mountPoint';
  }

  protected iconOf(node: DiskNode): IconName {
    return node.kind === 'unreadable' ? 'lock' : node.kind === 'mountPoint' ? 'disk' : node.kind === 'file' ? 'file' : 'folder';
  }

  /** The tag beside a name: why a refused folder was refused (visible words, not only a tooltip), or that part of a folder is missing. */
  protected tagText(node: DiskNode): string {
    return node.kind === 'unreadable' ? unreadableShort(node.unreadableReason) : 'Incomplete';
  }

  /** The segment the chart's menu was opened for: focus goes back to it when the menu closes (the chart has no method for this yet). */
  private chartMenuFor: string | null = null;

  protected refocusChart(): void {
    const id = this.chartMenuFor;
    this.chartMenuFor = null;
    if (id) this.hostEl.querySelector<SVGElement>(`pd-sunburst [data-id="${id}"]`)?.focus();
  }

  /** The chart's right-click or Shift+F10: the same menu as a row's, for the folder or file under it. */
  protected onChartMenu(e: { point: PdSunburstPoint; x: number; y: number }): void {
    const n = nodeOfChartId(e.point.node.id);
    if (n === null) return;
    const rings = this.store.rings();
    const node = rings ? findRingNode(rings, n) : null;
    if (!node) return;
    const trigger = this.chartMenu();
    this.chartMenuFor = e.point.node.id;
    this.menuAt.set({ x: e.x, y: e.y });
    this.cdr.detectChanges();
    trigger.menuData = { row: { node, drillable: node.hasChildren && node.kind === 'folder' } };
    trigger.openMenu();
  }

  protected async copyPath(node: number): Promise<void> {
    try {
      const path = await this.store.pathOf(node);
      if (path === null) return;
      await navigator.clipboard.writeText(path);
      this.notifier.show('Path copied');
    } catch {
      this.notifier.show('The path could not be copied. Select it and copy by hand.', 'bad');
    }
  }

  protected async copySize(node: DiskNode): Promise<void> {
    try {
      await navigator.clipboard.writeText(formatBytes(node.bytes));
      this.notifier.show('Size copied');
    } catch {
      this.notifier.show('The size could not be copied.', 'bad');
    }
  }

  /** Alt+Up or Cmd+Up goes up a folder (Back as well), Cmd+Shift+Up or Ctrl+Home to the root. Typing in the filter is left alone. */
  protected key(e: KeyboardEvent): void {
    const t = e.target as HTMLElement;
    if (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA') return;
    const mod = e.metaKey || e.altKey;
    if ((e.key === 'ArrowUp' && mod && e.shiftKey && e.metaKey) || (e.key === 'Home' && e.ctrlKey)) {
      e.preventDefault();
      void this.store.root();
    } else if ((e.key === 'ArrowUp' && mod) || (e.key === 'ArrowLeft' && e.altKey) || (e.key === '[' && e.metaKey)) {
      e.preventDefault();
      void this.store.up();
    }
  }
}
