import { inject, provideAppInitializer } from '@angular/core';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';

/**
 * Pitcrew's own icons, registered as literals in the `pc` namespace (`<mat-icon svgIcon="pc:refresh">`). No icon
 * fonts and nothing fetched at run time: the CSP allows neither. Icons use `currentColor`, so they follow the theme.
 */
export const ICONS: Record<string, string> = {
  refresh:
    '<svg viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" d="M20 12a8 8 0 1 1-2.34-5.66M20 4v5h-5"/></svg>',
  runner:
    '<svg viewBox="0 0 24 24"><circle cx="12" cy="12" r="8" fill="none" stroke="currentColor" stroke-width="2"/><path fill="currentColor" d="M10 8.5v7l6-3.5z"/></svg>',
  alert:
    '<svg viewBox="0 0 24 24"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linejoin="round" d="M12 3 2 21h20zM12 10v5M12 18v.5"/></svg>',
};

/** Registers {@link ICONS} at start-up. */
export function provideIcons() {
  return provideAppInitializer(() => {
    const registry = inject(MatIconRegistry);
    const sanitizer = inject(DomSanitizer);
    for (const [name, svg] of Object.entries(ICONS)) {
      // Our own constant markup, never data from a runner or the network.
      registry.addSvgIconLiteralInNamespace('pc', name, sanitizer.bypassSecurityTrustHtml(svg));
    }
  });
}
