import { ChangeDetectionStrategy, Component, computed, inject, InjectionToken, input, type Provider, type Type } from '@angular/core';
import { NgComponentOutlet } from '@angular/common';
import { MatIconModule, MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { ensurePaddockIcons, isPdIcon, pdIconId } from '@decryp7/paddock-ui/icons';

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

/** The shell's default icon: a `pd:` glyph (`<mat-icon svgIcon="pd:...">` over the set `providePaddockIcons` registered, so it has the same stroke and solid dots everywhere), or a letter on a rounded square for any other name. Decorative. */
@Component({
  selector: 'pd-shell-icon',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [MatIconModule],
  host: { 'aria-hidden': 'true' },
  template: `
    @if (glyph()) {
      <mat-icon [svgIcon]="glyph()" />
    } @else {
      <span class="pd-letter">{{ letter() }}</span>
    }
  `,
  styles: `
    mat-icon { width: 16px; height: 16px; }
    :host { display: inline-flex; width: 16px; height: 16px; flex: none; align-items: center; justify-content: center; }
    .pd-letter { display: inline-flex; align-items: center; justify-content: center; width: 16px; height: 16px; border-radius: var(--pd-radius-tag); background: var(--pd-surface-3); border: 1px solid var(--pd-border-strong); font-size: var(--pd-text-caption); line-height: 1; font-weight: var(--pd-weight-semibold); }
  `,
})
export class PdShellIcon {
  readonly name = input.required<string>();
  protected readonly glyph = computed(() => (this.name().startsWith('pd:') && isPdIcon(this.name()) ? pdIconId(this.name()) : ''));
  protected readonly letter = computed(() => iconLetter(this.name()));

  constructor() {
    ensurePaddockIcons(inject(MatIconRegistry), inject(DomSanitizer), true);
  }
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
