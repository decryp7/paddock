import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import { DialogActionError, type DialogAction, type DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/** The demo's save: `?unsaved=slow` never finishes (the busy state), `?unsaved=refuse` is refused (the rejected state). */
function save(search: string = globalThis.location?.search ?? ''): Promise<'save'> {
  const mode = new URLSearchParams(search).get('unsaved');
  if (mode === 'slow') return new Promise(() => undefined);
  if (mode === 'refuse') return Promise.reject(new DialogActionError('The greeting file is read-only. Save it somewhere else.'));
  return Promise.resolve('save');
}

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
    { kind: 'answer', id: 'save', label: 'Save', role: 'primary', answer: () => save() },
    { kind: 'answer', id: 'dont-save', label: 'Don’t save', role: 'destructive', answer: () => 'discard' },
  ]);
}
