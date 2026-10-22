import { ChangeDetectionStrategy, Component, input, output } from '@angular/core';

export type SuTrayPlatform = 'macos' | 'windows' | 'linux';

/**
 * Layout of a tray / menu-bar popup: 360 px wide, header, content fitted to its height (no empty gap, scrolls past 520 px)
 * and a footer toolbar. The window itself is frameless and transparent; this component draws the rounded surface
 * (12 px on macOS, 8 px on Windows 11), hairline border and soft shadow. Escape emits `dismissed`.
 *
 * Slots: `[slot=header]`, default (content), `[slot=footer]`.
 */
@Component({
  selector: 'su-tray-popover',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: {
    role: 'dialog',
    '[attr.aria-label]': 'label()',
    '[class]': '"su-tray su-tray--" + platform() + (translucent() ? " su-tray--translucent" : "")',
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
      width: var(--su-tray-width);
      max-width: 100%;
      max-height: var(--su-tray-max-height);
      overflow: hidden;
      background: var(--su-surface-3);
      border: 1px solid var(--su-hairline);
      border-radius: 12px;
      box-shadow: var(--su-shadow-2);
      color: var(--su-text);
      font-size: var(--su-text-body);
      animation: su-tray-in var(--su-duration) var(--su-ease-enter);
    }
    :host(.su-tray--windows) { border-radius: 8px; }
    :host(.su-tray--linux) { border-radius: var(--su-radius-popover); }
    /* The OS material (vibrancy, Acrylic) shows through; the solid colour is the designed fallback. */
    :host(.su-tray--translucent) { background: color-mix(in srgb, var(--su-surface-3) 72%, transparent); }
    .header { display: flex; align-items: center; gap: var(--su-space-2); padding: var(--su-space-3) var(--su-space-4); }
    .header:empty, .footer:empty { display: none; }
    .content { flex: 1 1 auto; min-height: 0; overflow-y: auto; padding: 0 var(--su-space-4) var(--su-space-3); }
    .footer { display: flex; align-items: center; gap: var(--su-space-2); padding: var(--su-space-2) var(--su-space-3); border-top: 1px solid var(--su-hairline); }
    @keyframes su-tray-in { from { opacity: 0; transform: translateY(4px); } to { opacity: 1; transform: none; } }
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
