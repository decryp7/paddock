import { ChangeDetectionStrategy, Component, inject, input, linkedSignal } from '@angular/core';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';

/** A form (modal here): a field, a pinned footer with Cancel and Rename; Enter in the field renames. */
@Component({
  selector: 'sd-rename-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <form class="form" (submit)="$event.preventDefault(); save()">
      <label class="field">
        <span>Name</span>
        <input type="text" name="name" [value]="name()" (input)="name.set($any($event.target).value)" autocomplete="off" />
      </label>
      <div class="actions">
        <button type="button" class="btn" data-dialog-close (click)="ref.close()">Cancel</button>
        <button type="submit" class="btn primary" [disabled]="!name().trim()">Rename</button>
      </div>
    </form>
  `,
  styleUrl: './dialog-content.css',
})
export class RenameDialog {
  /** The name shown at first (the opener's `inputs.current`). */
  readonly current = input('Shell demo');
  protected readonly name = linkedSignal(() => this.current());
  protected readonly ref = inject<DialogRef<string>>(DIALOG_REF);

  protected save(): void {
    const name = this.name().trim();
    if (name) this.ref.close(name);
  }
}
