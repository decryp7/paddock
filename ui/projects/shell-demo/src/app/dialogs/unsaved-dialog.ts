import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/**
 * A save prompt: Save (the primary), Don't save (destructive, drawn outlined beside a primary) and Cancel. The frame
 * orders them per OS (macOS and Linux: Don't save at the leading edge; Windows: Save, Don't save, Cancel).
 */
@Component({
  selector: 'sd-unsaved-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p class="text">Your changes to the greeting are lost if you don’t save them.</p>`,
  styleUrl: './dialog-content.css',
})
export class UnsavedDialog implements DialogContent<'save' | 'discard'> {
  readonly dialogHeading = signal('Save the changes to “Greeting”?');
  readonly dialogActions = signal<readonly DialogAction<'save' | 'discard'>[]>([
    { kind: 'answer', id: 'save', label: 'Save', role: 'primary', answer: () => 'save' },
    { kind: 'answer', id: 'dont-save', label: 'Don’t save', role: 'destructive', answer: () => 'discard' },
  ]);
}
