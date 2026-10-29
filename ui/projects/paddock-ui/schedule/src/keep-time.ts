import { DestroyRef, Directive, ElementRef, inject } from '@angular/core';
import { MatTimepickerInput } from '@angular/material/timepicker';

/**
 * Escape in a time field whose list is shut must not clear the time. Material's `mat-timepicker` input does clear it (Escape is its
 * "empty this field" key) and calls `preventDefault`, which would also make a dialog around the editor think the key was used.
 * Here the key is left alone for the dialog, and the picker is told it was not an Escape:
 *
 * - a capture listener on the input (it runs before Material's own) hides the key code Material looks at (`keyCode`, 27 for Escape);
 * - a one-shot bubble listener, added during that capture pass so that it runs after every listener Material put on the input, puts the
 *   real `keyCode` back, so nothing above the input is told anything wrong: the CDK dialog, `MatDialog` and the overlay keyboard
 *   dispatcher decide Escape with `keyCode === ESCAPE`.
 *
 * With the list open Escape is the editor's own (`closePicker`) and is not touched. This leans on Material's input reading
 * `event.keyCode` in its own keydown handler; checked against Material 22.2.4 (`MatTimepickerInput._handleKeydown`).
 */
@Directive({ selector: 'input[matTimepicker][pdKeepTime]' })
export class PdKeepTimeOnEscape {
  constructor() {
    const el = inject<ElementRef<HTMLInputElement>>(ElementRef).nativeElement;
    const input = inject(MatTimepickerInput);
    let pending = false;
    const restore = (e: KeyboardEvent): void => {
      pending = false;
      delete (e as { keyCode?: number }).keyCode;
    };
    const hide = (e: KeyboardEvent): void => {
      if (e.key !== 'Escape' || input.timepicker().isOpen()) return;
      Object.defineProperty(e, 'keyCode', { value: 0, configurable: true });
      if (!pending) {
        pending = true;
        el.addEventListener('keydown', restore, { once: true });
      }
    };
    el.addEventListener('keydown', hide, true);
    inject(DestroyRef).onDestroy(() => {
      el.removeEventListener('keydown', hide, true);
      el.removeEventListener('keydown', restore);
    });
  }
}
