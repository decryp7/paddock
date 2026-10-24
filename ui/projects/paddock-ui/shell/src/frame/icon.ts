import { ChangeDetectionStrategy, Component, computed, inject, InjectionToken, input, type Provider, type Type } from '@angular/core';
import { NgComponentOutlet } from '@angular/common';
import { PD_ICON_PATHS, type PdIconName } from '@decryp7/paddock-ui/icons';

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

/** The letter a name without an icon is drawn with: the first letter of its last part (`pw:models` gives M). */
export function iconLetter(name: string): string {
  const tail = name.split(/[:./]/).filter(Boolean).at(-1) ?? '';
  return (tail.match(/[\p{L}\p{N}]/u)?.[0] ?? '?').toUpperCase();
}

/** The path of a `pd:` glyph (the generic set of `@decryp7/paddock-ui/icons`), or null for any other name. */
function pdIconPath(name: string): string | null {
  return name.startsWith('pd:') ? (PD_ICON_PATHS[name.slice(3) as PdIconName] ?? null) : null;
}

/** The shell's default icon: a `pd:` glyph (24 px grid drawn at 16 px with a heavier optical stroke), or a letter on a rounded square for any other name. Decorative. */
@Component({
  selector: 'pd-shell-icon',
  changeDetection: ChangeDetectionStrategy.OnPush,
  host: { 'aria-hidden': 'true' },
  template: `
    @if (path(); as d) {
      <svg viewBox="0 0 24 24" width="16" height="16" focusable="false">
        <path [attr.d]="d" fill="none" stroke="currentColor" stroke-width="2.25" stroke-linecap="round" stroke-linejoin="round" />
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
  protected readonly path = computed(() => pdIconPath(this.name()));
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
