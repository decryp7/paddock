import { ChangeDetectionStrategy, Component, computed, input, linkedSignal } from '@angular/core';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/** A form (modal here): a field, and the frame's footer with Cancel and Rename; Enter in the field renames. */
@Component({
  selector: 'sd-rename-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <form class="form" (submit)="$event.preventDefault()">
      <label class="field">
        <span>Name</span>
        <input type="text" name="name" data-autofocus [value]="name()" (input)="name.set($any($event.target).value)" autocomplete="off" />
      </label>
    </form>
  `,
  styleUrl: './dialog-content.css',
})
export class RenameDialog implements DialogContent<string> {
  /** The name shown at first (the opener's `inputs.current`). */
  readonly current = input('Shell demo');
  protected readonly name = linkedSignal(() => this.current());

  /** Rename, enabled while the name is not blank. */
  readonly dialogActions = computed<readonly DialogAction<string>[]>(() => [
    { kind: 'answer', id: 'rename', label: 'Rename', role: 'primary', enabled: this.name().trim() !== '', answer: () => this.name().trim() },
  ]);
}
