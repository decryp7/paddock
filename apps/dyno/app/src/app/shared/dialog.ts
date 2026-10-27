import { DIALOG_DATA, Dialog, type DialogConfig, DialogRef } from '@angular/cdk/dialog';
import { ChangeDetectionStrategy, Component, Injectable, inject, input } from '@angular/core';
import { firstValueFrom } from 'rxjs';
import { Icon } from './icon';
import type { IconName } from './icons';

/** The frame of every Dyno dialog: title bar with a close button, a scrolling body and a footer for actions. */
@Component({
  selector: 'dy-dialog',
  imports: [Icon],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <header>
      @if (icon(); as i) {
        <span class="icon" [attr.data-tone]="tone()"><dy-icon [name]="i" /></span>
      }
      <div class="titles">
        <h2 [id]="titleId()">{{ title() }}</h2>
        @if (subtitle()) {
          <p>{{ subtitle() }}</p>
        }
      </div>
      <button type="button" class="btn ghost icon sm" aria-label="Close" (click)="ref.close()"><dy-icon name="close" /></button>
    </header>
    <div class="body"><ng-content /></div>
    <footer><ng-content select="[footer]" /></footer>
  `,
  styles: `
    :host { display: flex; flex-direction: column; width: var(--dialog-w, 480px); max-width: 100%; max-height: calc(100vh - 48px); background: var(--overlay-bg); border: 1px solid var(--border); border-radius: var(--r-xl); box-shadow: var(--shadow-3); overflow: hidden; animation: dy-fade-in var(--dur-base) var(--ease-out); }
    header { display: flex; align-items: flex-start; gap: var(--sp-3); padding: var(--sp-4) var(--sp-4) var(--sp-3) var(--sp-5); }
    .icon { display: grid; place-items: center; width: 32px; height: 32px; border-radius: var(--r-md); background: var(--accent-soft); color: var(--accent-text); flex: none; }
    .icon[data-tone='bad'] { background: var(--bad-soft); color: var(--bad); }
    .icon[data-tone='warn'] { background: var(--warn-soft); color: var(--warn); }
    .titles { flex: 1; min-width: 0; display: grid; gap: 2px; padding-top: 5px; }
    h2 { font-size: var(--fs-md); }
    .titles p { color: var(--text-2); font-size: var(--fs-sm); }
    .body { padding: 0 var(--sp-5) var(--sp-4); overflow: auto; display: grid; gap: var(--sp-3); }
    footer { display: flex; justify-content: flex-end; gap: var(--sp-2); padding: var(--sp-3) var(--sp-5); border-top: 1px solid var(--border); background: var(--surface-inset); }
    footer:empty { display: none; }
  `,
})
export class DialogFrame {
  protected readonly ref = inject(DialogRef);
  readonly title = input.required<string>();
  readonly subtitle = input('');
  readonly icon = input<IconName | null>(null);
  readonly tone = input<'accent' | 'bad' | 'warn'>('accent');
  readonly titleId = input('dy-dialog-title');
}

export interface ConfirmOptions {
  title: string;
  message: string;
  confirmLabel: string;
  danger?: boolean;
  icon?: IconName;
  /** Extra lines (what exactly will happen). */
  details?: string[];
}

/** A yes/no question. Resolves `true` only for the confirm button. */
@Component({
  selector: 'dy-confirm-dialog',
  imports: [DialogFrame],
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <dy-dialog [title]="data.title" [icon]="data.icon ?? (data.danger ? 'alert' : 'info')" [tone]="data.danger ? 'bad' : 'accent'">
      <p class="msg">{{ data.message }}</p>
      @if (data.details?.length) {
        <ul>
          @for (d of data.details; track $index) {
            <li>{{ d }}</li>
          }
        </ul>
      }
      <ng-container footer>
        <button type="button" class="btn" (click)="ref.close(false)">Cancel</button>
        <button type="button" class="btn" [class.primary]="!data.danger" [class.danger-solid]="data.danger" (click)="ref.close(true)">{{ data.confirmLabel }}</button>
      </ng-container>
    </dy-dialog>
  `,
  styles: `
    .msg { color: var(--text-2); }
    ul { margin: 0; padding-left: var(--sp-5); color: var(--text-2); display: grid; gap: var(--sp-1); font-size: var(--fs-sm); }
  `,
})
export class ConfirmDialog {
  protected readonly data = inject<ConfirmOptions>(DIALOG_DATA);
  protected readonly ref = inject<DialogRef<boolean>>(DialogRef);
}

/** Opens Dyno's dialogs with the shared look (backdrop, panel class, focus trap and restore from the CDK). */
@Injectable({ providedIn: 'root' })
export class Dialogs {
  private readonly dialog = inject(Dialog);

  open<R, D = unknown>(component: unknown, data?: D, config: Partial<DialogConfig<D, DialogRef<R, unknown>>> = {}): DialogRef<R, unknown> {
    return this.dialog.open<R, D, unknown>(component as never, {
      data,
      panelClass: 'dy-dialog-panel',
      backdropClass: 'dy-backdrop',
      autoFocus: 'first-tabbable',
      restoreFocus: true,
      ariaLabelledBy: 'dy-dialog-title',
      ...config,
    });
  }

  async confirm(options: ConfirmOptions): Promise<boolean> {
    const ref = this.open<boolean, ConfirmOptions>(ConfirmDialog, options);
    return (await firstValueFrom(ref.closed)) === true;
  }
}
