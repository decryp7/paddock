import { ChangeDetectionStrategy, Component, computed, inject, InjectionToken, input, type Provider, type Type } from '@angular/core';
import { NgComponentOutlet } from '@angular/common';

/**
 * The component that draws a contribution's icon by name (`pw:models`, `pd:home`): it has a `name` input. An app passes
 * its own, which typically wraps Angular Material's registry (`<mat-icon [svgIcon]="name()" />`), so the shell does not
 * depend on Material; without one the shell draws its own `pd:` set and a letter for any other name.
 */
export const SHELL_ICON = new InjectionToken<Type<unknown>>('PADDOCK_SHELL_ICON', { providedIn: 'root', factory: () => PdShellIcon });

/** Uses `component` (it has a `name` input) for the icons of pages, status items and the frame. */
export function provideShellIcon(component: Type<unknown>): Provider {
  return { provide: SHELL_ICON, useValue: component };
}

/** The shell's own icons: 16 px grid, 1.5 px stroke, round caps and joins (design guide §6). */
export const SHELL_ICON_PATHS: Readonly<Record<string, string>> = {
  'pd:home': 'M2.5 7.5 8 3l5.5 4.5M4 6.5V13h3V9.5h2V13h3V6.5',
  'pd:settings':
    'M8 5.75a2.25 2.25 0 1 0 0 4.5 2.25 2.25 0 0 0 0-4.5ZM8 1.75v2M8 12.25v2M1.75 8h2M12.25 8h2M3.6 3.6 5 5M11 11l1.4 1.4M3.6 12.4 5 11M11 5l1.4-1.4',
  'pd:info': 'M8 14.25a6.25 6.25 0 1 0 0-12.5 6.25 6.25 0 0 0 0 12.5ZM8 7.5V11M8 5h.01',
  'pd:menu': 'M2.5 4h11M2.5 8h11M2.5 12h11',
  'pd:sidebar': 'M2.25 3h11.5v10H2.25ZM6 3v10',
  'pd:window': 'M2 3.25h12v9.5H2ZM2 6h12',
  'pd:keyboard': 'M1.5 4.5h13v7h-13ZM4 7h.01M6.5 7h.01M9 7h.01M11.5 7h.01M4.5 9.5h7',
  'pd:activity': 'M1.5 8h3l2-4.5 3 9 2-4.5h3',
  'pd:chevron-left': 'M10 3.5 5.5 8l4.5 4.5',
  'pd:chevron-right': 'M6 3.5 10.5 8 6 12.5',
  'pd:more': 'M3.5 8h.01M8 8h.01M12.5 8h.01',
};

/** The letter a name without an icon is drawn with: the first letter of its last part (`pw:models` gives M). */
export function iconLetter(name: string): string {
  const tail = name.split(/[:./]/).filter(Boolean).at(-1) ?? '';
  return (tail.match(/[\p{L}\p{N}]/u)?.[0] ?? '?').toUpperCase();
}

/** The shell's default icon: a `pd:` path, or a letter on a rounded square for any other name. Decorative. */
@Component({
  selector: 'pd-shell-icon',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'aria-hidden': 'true' },
  template: `
    @if (path(); as d) {
      <svg viewBox="0 0 16 16" width="16" height="16" focusable="false">
        <path [attr.d]="d" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
      </svg>
    } @else {
      <span class="pd-letter">{{ letter() }}</span>
    }
  `,
  styles: `
    :host { display: inline-flex; width: 16px; height: 16px; flex: none; align-items: center; justify-content: center; }
    svg { display: block; }
    .pd-letter { display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 16px; border-radius: var(--pd-radius-tag); background: var(--pd-surface-3); border: 1px solid var(--pd-border-strong); font-size: 10px; line-height: 1; font-weight: var(--pd-weight-semibold); }
  `,
})
export class PdShellIcon {
  readonly name = input.required<string>();
  protected readonly path = computed(() => SHELL_ICON_PATHS[this.name()] ?? null);
  protected readonly letter = computed(() => iconLetter(this.name()));
}

/** Draws an icon by name with the window's icon component (`SHELL_ICON`). */
@Component({
  selector: 'pd-icon',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgComponentOutlet],
  host: { 'aria-hidden': 'true' },
  template: `<ng-container *ngComponentOutlet="component; inputs: { name: name() }" />`,
  styles: `:host { display: inline-flex; flex: none; align-items: center; justify-content: center; }`,
})
export class PdIcon {
  readonly name = input.required<string>();
  protected readonly component = inject(SHELL_ICON);
}
