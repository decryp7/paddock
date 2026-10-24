import { ChangeDetectionStrategy, Component } from '@angular/core';

/** A viewer: modeless, opens at 680x560 and only grows. It has no button of its own: the dialog shell adds Close. */
@Component({
  selector: 'sd-about-dialog',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <article class="text">
      <h2>Paddock shell demo</h2>
      <p>The reference app of the Paddock app shell. Every shell part shows up here first, against a mock backend.</p>
      <p>This window is a viewer: it stays open beside the work, and Escape or Close dismisses it.</p>
    </article>
  `,
  styleUrl: './dialog-content.css',
})
export class AboutDialog {}
