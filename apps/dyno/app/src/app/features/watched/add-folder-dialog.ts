import { DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import type { AddWatchedFolder, WatchTemplateInfo } from '../../core/api.types';
import { DialogFrame } from '../../shared/dialog';
import { Icon } from '../../shared/icon';
import { WatchedStore } from '../../state/watched.store';
import { KEEP_PRESETS, keepLabel } from './watched-model';

/**
 * "Add folder": the kind of files and how long they may stay, then the OS folder dialog (the folder is only ever chosen there).
 * A new folder starts switched off; it can be turned on after its preview. Nothing is moved by adding.
 */
@Component({
  selector: 'dy-add-folder-dialog',
  imports: [DialogFrame, FormsModule, Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog title="Watch a folder" subtitle="Files older than the time you set move to the 7-day quarantine" icon="folder">
      <div class="field" role="radiogroup" aria-label="What to watch">
        <span class="label">What to watch</span>
        <div class="choices">
          @for (t of templates(); track t.id) {
            <button type="button" class="choice" role="radio" [attr.aria-checked]="template() === t.id" (click)="pick(t)">
              <strong>{{ t.name }}</strong>
              <small>{{ t.about }}</small>
            </button>
          }
          <button type="button" class="choice" role="radio" [attr.aria-checked]="template() === null" (click)="pick(null)">
            <strong>Any files</strong>
            <small>Every file in the folder you pick.</small>
          </button>
        </div>
      </div>
      <label class="field">
        <span class="label">Keep files for</span>
        <select class="select" name="keep" [ngModel]="keep()" (ngModelChange)="keep.set(+$event)" aria-label="Keep files for">
          @for (p of presets; track p.secs) {
            <option [ngValue]="p.secs">{{ p.label }}</option>
          }
        </select>
      </label>
      <label class="check"><input type="checkbox" [ngModel]="recursive()" (ngModelChange)="recursive.set($event)" name="recursive" /> Include subfolders</label>
      <p class="note"><dy-icon name="info" /> Dyno will ask you to choose the folder next. It starts switched off; you will see exactly which files would move before you turn it on.</p>
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close()">Cancel</button>
        <button type="button" class="btn primary" [disabled]="store.busy() !== null" (click)="choose()"><dy-icon name="folder" /> Choose folder…</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    :host { --dialog-w: 540px; }
    .choices { display: grid; gap: var(--sp-2); }
    .choice { display: grid; gap: 2px; text-align: left; padding: var(--sp-2) var(--sp-3); border: 1px solid var(--border); border-radius: var(--r-md); background: var(--surface-inset); color: var(--text-1); cursor: pointer; font: inherit; }
    .choice small { color: var(--text-3); font-size: var(--fs-sm); }
    .choice:hover { border-color: var(--border-strong); }
    .choice[aria-checked='true'] { border-color: var(--accent); background: var(--accent-soft); }
    .check { display: flex; align-items: center; gap: var(--sp-2); color: var(--text-2); font-size: var(--fs-sm); }
    .note { margin: 0; display: flex; gap: var(--sp-2); align-items: flex-start; color: var(--text-3); font-size: var(--fs-sm); }
    .note dy-icon { --icon-size: 14px; margin-top: 2px; flex: none; }
  `,
})
export class AddFolderDialog {
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
  protected readonly store = inject(WatchedStore);
  protected readonly presets = KEEP_PRESETS;
  protected readonly templates = computed<WatchTemplateInfo[]>(() => this.store.overview()?.templates ?? []);
  protected readonly template = signal<string | null>(null);
  protected readonly keep = signal(7 * 86_400);
  protected readonly recursive = signal(false);
  protected readonly label = keepLabel;

  protected pick(t: WatchTemplateInfo | null): void {
    this.template.set(t?.id ?? null);
    if (t) {
      this.keep.set(t.keepForSecs);
      this.recursive.set(t.recursive);
    }
  }

  protected async choose(): Promise<void> {
    const request: AddWatchedFolder = { template: this.template(), keepForSecs: this.keep(), recursive: this.recursive() };
    // The OS folder dialog opens in the shell; it resolves with false when they cancel, and the dialog stays for another try.
    if (await this.store.add(request)) this.ref.close(true);
  }
}
