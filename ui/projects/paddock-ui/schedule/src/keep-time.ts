import { DestroyRef, Directive, ElementRef, inject } from '@angular/core';
import { MatTimepickerInput } from '@angular/material/timepicker';

/**
 * Escape in a time field whose list is shut must not clear the time. Material's `mat-timepicker` input does clear it (Escape is its
 * "empty this field" key) and calls `preventDefault`, which would also make a dialog around the editor think the key was used.
 * Here the key is left alone for the dialog (which reads `key`), and the picker is told it was not an Escape: a capture listener
 * on the input itself, which runs before Material's, hides the key code Material looks at (`keyCode`, 27 for Escape).
 * With the list open Escape is the editor's own (`closePicker`) and is not touched.
 */
@Directive({ selector: 'input[matTimepicker][pdKeepTime]' })
export class PdKeepTimeOnEscape {
  constructor() {
    const el = inject<ElementRef<HTMLInputElement>>(ElementRef).nativeElement;
    const input = inject(MatTimepickerInput);
    const hide = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && !input.timepicker().isOpen()) Object.defineProperty(e, 'keyCode', { value: 0 });
    };
    el.addEventListener('keydown', hide, true);
    inject(DestroyRef).onDestroy(() => el.removeEventListener('keydown', hide, true));
  }
}
