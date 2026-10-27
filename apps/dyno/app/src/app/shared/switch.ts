import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

/** An on/off switch (a real button with `role="switch"`): space and enter toggle it, the label comes from `label`. */
@Component({
  selector: 'dy-switch',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <button type="button" role="switch" [attr.aria-checked]="checked()" [attr.aria-label]="label()" [disabled]="disabled()" (click)="changed.emit(!checked())">
      <span class="knob"></span>
    </button>
  `,
  styles: `
    :host { display: inline-flex; }
    button { position: relative; width: 36px; height: 20px; padding: 0; border: 1px solid var(--border-strong); border-radius: var(--r-pill); background: var(--surface-active); cursor: default; transition: background var(--dur-fast) var(--ease-out), border-color var(--dur-fast) var(--ease-out); }
    button[aria-checked='true'] { background: var(--accent); border-color: transparent; }
    button:disabled { opacity: 0.5; }
    .knob { position: absolute; top: 2px; left: 2px; width: 14px; height: 14px; border-radius: 50%; background: var(--text-3); transition: transform var(--dur-fast) var(--ease-out), background var(--dur-fast) var(--ease-out); }
    button[aria-checked='true'] .knob { transform: translateX(16px); background: var(--text-on-accent); }
  `,
})
export class Switch {
  readonly checked = input.required<boolean>();
  readonly label = input.required<string>();
  readonly disabled = input(false);
  readonly changed = output<boolean>();
}
