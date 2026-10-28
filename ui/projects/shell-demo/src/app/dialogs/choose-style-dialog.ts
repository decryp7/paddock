import { ChangeDetectionStrategy, Component, signal } from '@angular/core';
import type { DialogAction, DialogContent } from '@decryp7/paddock-ui/shell/dialogs';

/** A required choice: a modal without Cancel. Escape and a close request leave it open; it closes with its answer. */
@Component({
  selector: 'sd-choose-style-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `<p class="text">You can change it later in Settings.</p>`,
  styleUrl: './dialog-content.css',
})
export class ChooseStyleDialog implements DialogContent<'casual' | 'formal'> {
  readonly dialogHeading = signal('How should the demo greet you?');
  readonly dialogActions = signal<readonly DialogAction<'casual' | 'formal'>[]>([
    { kind: 'answer', id: 'casual', label: 'Casual', role: 'secondary', answer: () => 'casual' },
    { kind: 'answer', id: 'formal', label: 'Formal', role: 'primary', answer: () => 'formal' },
  ]);
}
