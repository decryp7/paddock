import { CdkContextMenuTrigger, CdkMenu, CdkMenuItem, CdkMenuTrigger } from '@angular/cdk/menu';
import { ChangeDetectionStrategy, Component, DestroyRef, OnInit, computed, inject } from '@angular/core';
import type { WatchedFolderView } from '../../core/api.types';
import { AsyncState } from '../../shared/async-state';
import { Dialogs } from '../../shared/dialog';
import { formatDate } from '../../shared/format';
import { Icon } from '../../shared/icon';
import { WatchedStore } from '../../state/watched.store';
import { AddFolderDialog } from './add-folder-dialog';
import { PreviewDialog, type PreviewData } from './preview-dialog';
import { KEEP_PRESETS, folderName, keepLabel, shortPath, statusOf, sweepText } from './watched-model';

/**
 * Watched folders: files older than a time you set move to the 7-day quarantine, only in the folders you choose. A new folder is
 * off until you have seen exactly what would move; nothing outside the folder is touched and everything can be put back.
 */
@Component({
  selector: 'dy-watched-page',
  imports: [AsyncState, Icon, CdkContextMenuTrigger, CdkMenu, CdkMenuItem, CdkMenuTrigger],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div class="page">
      <header class="page-head">
        <div class="titles">
          <h1>Watched folders</h1>
          <p>Files older than the time you set move to the 7-day quarantine. Only in folders you choose; everything can be put back.</p>
        </div>
        @if (rows().length > 0) {
          <button type="button" class="btn primary" (click)="add()"><dy-icon name="plus" /> Add folder</button>
        }
      </header>

      <dy-async [loaded]="!!store.overview()" [error]="store.error()" [empty]="rows().length === 0" emptyIcon="folder" emptyTitle="No folders watched" emptyText="Pick a folder such as Downloads and how long files may stay in it. Dyno shows what would move before anything does." what="watched folders" [rows]="3" (retry)="store.load()">
        <div empty-action><button type="button" class="btn primary" (click)="add()"><dy-icon name="plus" /> Add folder</button></div>
        <div class="tablewrap">
          <table class="table folders" aria-label="Watched folders">
            <thead><tr><th>Folder</th><th>Applies to</th><th class="keep">Keep for</th><th>Last sweep</th><th class="state">State</th><th class="acts"><span class="visually-hidden">Actions</span></th></tr></thead>
            <tbody>
              @for (v of rows(); track v.folder.id) {
                <tr class="row" tabindex="0" [attr.data-id]="v.folder.id" [cdkContextMenuTriggerFor]="menu" [cdkContextMenuTriggerData]="{ v }" (keydown.enter)="preview(v)" (keydown.shift.f10)="$event.preventDefault()" (dblclick)="preview(v)">
                  <td>
                    <span class="fold"><dy-icon name="folder" class="fi" /><span class="ell"><strong>{{ name(v) }}</strong><small class="path" [title]="v.folder.path">{{ short(v.folder.path) }}</small></span></span>
                  </td>
                  <td><span class="ell" [title]="v.filterText">{{ v.filterText }}@if (v.folder.recursive) { <span class="muted"> · subfolders</span> }</span></td>
                  <td class="keep num">{{ keep(v) }}</td>
                  <td><span class="ell muted" [title]="sweep(v)">{{ sweep(v) }}</span></td>
                  <td class="state"><span class="st"><span class="dot" [attr.data-tone]="status(v).tone" aria-hidden="true"></span>{{ status(v).text }}</span></td>
                  <td class="acts">
                    <button type="button" class="ib" [attr.aria-label]="'See what would move in ' + name(v)" title="What would move" (click)="preview(v)"><dy-icon name="eye" /></button>
                    <button type="button" class="ib more" [attr.aria-label]="'Actions for ' + name(v)" [cdkMenuTriggerFor]="menu" [cdkMenuTriggerData]="{ v }"><dy-icon name="more" /></button>
                  </td>
                </tr>
              }
            </tbody>
          </table>
        </div>
        <p class="foot">Dyno checks each folder every few minutes. A file counts from the later of its creation and last change. Files in use are skipped, and links are never followed.</p>
      </dy-async>
    </div>

    <ng-template #menu let-v="v">
      <div class="menu wide" cdkMenu [attr.aria-label]="'Actions for ' + name(v)">
        <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="preview(v)"><dy-icon name="eye" /> See what would move…</button>
        @if (v.folder.enabled) {
          <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="turn(v, false)"><dy-icon name="pause" /> Turn off</button>
        } @else {
          <button type="button" class="menu-item" cdkMenuItem [cdkMenuItemDisabled]="!v.exists" (cdkMenuItemTriggered)="turnOn(v)"><dy-icon name="play" /> Turn on…</button>
        }
        <button type="button" class="menu-item" cdkMenuItem [cdkMenuTriggerFor]="keepMenu" [cdkMenuTriggerData]="{ v }"><dy-icon name="clock" /> Keep for <dy-icon name="chevron-right" class="trail" /></button>
        <div class="menu-sep"></div>
        <button type="button" class="menu-item danger" cdkMenuItem (cdkMenuItemTriggered)="remove(v)"><dy-icon name="trash" /> Stop watching…</button>
      </div>
    </ng-template>
    <ng-template #keepMenu let-v="v">
      <div class="menu" cdkMenu aria-label="Keep for">
        @for (p of presets; track p.secs) {
          <button type="button" class="menu-item" cdkMenuItem (cdkMenuItemTriggered)="setKeep(v, p.secs)">{{ p.label }} @if (p.secs === v.folder.keepForSecs) { <dy-icon name="check" class="trail" /> }</button>
        }
      </div>
    </ng-template>
  `,
  styles: `
    :host { display: block; }
    .page { display: grid; gap: var(--sp-3); align-content: start; }
    dy-async { display: grid; gap: var(--sp-3); }
    .tablewrap { border: 1px solid var(--border); border-radius: var(--r-lg); background: var(--surface-2); overflow: auto; }
    .folders { table-layout: fixed; }
    .folders th.keep, .folders td.keep { width: 96px; text-align: right; }
    .folders th.state, .folders td.state { width: 190px; }
    .folders th.acts, .folders td.acts { width: 72px; padding: 0 4px; text-align: right; }
    .folders td { overflow: hidden; text-overflow: ellipsis; }
    .folders th:first-child, .folders td:first-child { width: 28%; }
    .folders td { height: 48px; }
    .fold { display: flex; align-items: center; gap: var(--sp-2); min-width: 0; }
    .fi { --icon-size: 16px; color: var(--text-3); flex: none; }
    .ell { display: block; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
    .ell strong { display: block; font-weight: var(--fw-medium); overflow: hidden; text-overflow: ellipsis; }
    .path { display: block; color: var(--text-3); font: var(--fw-regular) 12px / 1.3 var(--font-mono); overflow: hidden; text-overflow: ellipsis; }
    .muted { color: var(--text-3); }
    .st { display: inline-flex; align-items: center; gap: var(--sp-2); white-space: nowrap; }
    .row { cursor: default; }
    .row:focus-visible { outline: 2px solid var(--focus); outline-offset: -2px; }
    td.acts { white-space: nowrap; }
    td.acts .ib { opacity: 0; transition: opacity var(--dur-fast) var(--ease-out); }
    td.acts .ib.more { opacity: 0.4; }
    .row:hover td.acts .ib, .row:focus-within td.acts .ib { opacity: 1; }
    .ib { display: inline-grid; place-items: center; width: 24px; height: 24px; border: 0; border-radius: var(--r-sm); background: none; color: var(--text-2); cursor: pointer; }
    .ib:hover { background: var(--surface-active); color: var(--text-1); }
    .ib dy-icon { --icon-size: 15px; }
    .foot { margin: 0; color: var(--text-3); font-size: var(--fs-sm); }
    .menu-item.danger { color: var(--bad); }
  `,
})
export class WatchedPage implements OnInit {
  protected readonly store = inject(WatchedStore);
  private readonly dialogs = inject(Dialogs);
  private readonly destroyRef = inject(DestroyRef);
  protected readonly presets = KEEP_PRESETS;
  protected readonly rows = computed<WatchedFolderView[]>(() => this.store.overview()?.folders ?? []);
  protected readonly name = (v: WatchedFolderView) => folderName(v.folder.path);
  protected readonly short = shortPath;
  protected readonly keep = (v: WatchedFolderView) => keepLabel(v.folder.keepForSecs);
  protected readonly status = statusOf;
  protected readonly when = (at: number) => formatDate(at, Date.now());

  ngOnInit(): void {
    void this.store.load();
    // Sweeps happen in the background: refresh the list now and then while the page is open.
    const timer = setInterval(() => void this.store.load(), 30_000);
    this.destroyRef.onDestroy(() => clearInterval(timer));
  }

  protected sweep(v: WatchedFolderView): string {
    return sweepText(v.folder.lastSweep, Date.now());
  }

  protected add(): void {
    this.dialogs.open<boolean>(AddFolderDialog);
  }

  protected preview(v: WatchedFolderView): void {
    this.dialogs.open<boolean, PreviewData>(PreviewDialog, { id: v.folder.id });
  }

  /** Turning on needs the preview first: this opens it, and its "Turn on" button does the rest. */
  protected turnOn(v: WatchedFolderView): void {
    this.preview(v);
  }

  protected turn(v: WatchedFolderView, enabled: boolean): void {
    void this.store.change(v.folder.id, { enabled }, enabled ? 'Watching the folder' : 'Stopped sweeping the folder');
  }

  protected setKeep(v: WatchedFolderView, secs: number): void {
    if (secs === v.folder.keepForSecs) return;
    const shorter = secs < v.folder.keepForSecs;
    void this.store.change(v.folder.id, { keepForSecs: secs }, shorter ? 'Keep time changed. Look at the preview again to turn the folder back on' : 'Keep time changed');
  }

  protected async remove(v: WatchedFolderView): Promise<void> {
    const ok = await this.dialogs.confirm({
      title: `Stop watching ${this.name(v)}?`,
      message: 'The files stay where they are. Anything already moved to the quarantine can still be put back from Activity.',
      confirmLabel: 'Stop watching',
      danger: true,
      details: [v.folder.path],
    });
    if (ok) await this.store.remove(v.folder.id);
  }
}
