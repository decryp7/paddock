import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

export type SuTrayPlatform = 'macos' | 'windows' | 'linux';

/**
 * Layout of a tray / menu-bar popup: 360 px wide, header, content fitted to its height (no empty gap, scrolls past 520 px)
 * and a footer toolbar. The window itself is frameless and transparent; this component draws the rounded surface
 * (12 px on macOS, 8 px on Windows 11), hairline border and soft shadow, one 16 px inset for header, content and footer.
 * Escape emits `dismissed`.
 *
 * Slots: `[slot=header]`, default (content), `[slot=footer]`.
 */
@Component({
  selector: 'pd-tray-popover',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    role: 'dialog',
    '[attr.aria-label]': 'label()',
    '[class]': '"pd-tray pd-tray--" + platform() + (translucent() ? " pd-tray--translucent" : "")',
    '(keydown.escape)': 'dismissed.emit()',
  },
  template: `
    <header class="header"><ng-content select="[slot=header]" /></header>
    <div class="content"><ng-content /></div>
    <footer class="footer"><ng-content select="[slot=footer]" /></footer>
  `,
  styles: `
    :host {
      display: flex;
      flex-direction: column;
      width: var(--pd-tray-width);
      max-width: 100%;
      max-height: var(--pd-tray-max-height);
      overflow: hidden;
      background: var(--pd-surface-3);
      border: 1px solid var(--pd-hairline);
      border-radius: 12px;
      box-shadow: var(--pd-shadow-2);
      color: var(--pd-text);
      font-size: var(--pd-text-body);
      animation: pd-tray-in var(--pd-duration) var(--pd-ease-enter);
    }
    :host(.pd-tray--windows) { border-radius: 8px; }
    :host(.pd-tray--linux) { border-radius: var(--pd-radius-popover); }
    /* The OS material (vibrancy, Acrylic) shows through; the solid colour is the designed fallback. */
    :host(.pd-tray--translucent) { background: color-mix(in srgb, var(--pd-surface-3) 72%, transparent); }
    .header { display: flex; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-3) var(--pd-space-4); }
    .header:empty, .footer:empty { display: none; }
    .content { flex: 1 1 auto; min-height: 0; overflow-y: auto; padding: 0 var(--pd-space-4) var(--pd-space-3); }
    .footer { display: flex; align-items: center; gap: var(--pd-space-2); padding: var(--pd-space-2) var(--pd-space-4); border-top: 1px solid var(--pd-hairline); }
    @keyframes pd-tray-in { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
    @media (prefers-reduced-motion: reduce) { :host { animation: none; } }
  `,
})
export class SuTrayPopover {
  /** Accessible name, e.g. "Dyno". */
  readonly label = input.required<string>();
  readonly platform = input<SuTrayPlatform>('macos');
  /** The shell draws an OS material behind the page; let it show through. */
  readonly translucent = input<boolean>(false);
  readonly dismissed = output<void>();
}
