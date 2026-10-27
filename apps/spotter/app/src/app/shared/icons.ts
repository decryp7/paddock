import { inject, provideAppInitializer } from '@angular/core';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';

/** A 24×24 outline icon from path data, drawn with `currentColor` so it follows the theme. */
function outline(body: string): string {
  return (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" ' +
    `stroke-linecap="round" stroke-linejoin="round">${body}</svg>`
  );
}

const p = (d: string) => `<path d="${d}"/>`;
const c = (cx: number, cy: number, r: number) => `<circle cx="${cx}" cy="${cy}" r="${r}"/>`;
const dot = (cx: number, cy: number) => `<circle cx="${cx}" cy="${cy}" r="1.1" fill="currentColor" stroke="none"/>`;

/**
 * Spotter's own icons, registered as literals in the `sp` namespace (`<mat-icon svgIcon="sp:refresh">`). No icon
 * fonts and nothing fetched at run time: the CSP allows neither.
 */
export const ICONS: Record<string, string> = {
  // Navigation
  dashboard: outline(p('M4 4h7v9H4zM13 4h7v5h-7zM13 11h7v9h-7zM4 15h7v5H4z')),
  watches: outline(p('M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z') + c(12, 12, 3)),
  settings: outline(
    c(12, 12, 3) + p('M12 2.8v2.4M12 18.8v2.4M4.2 7.5l2.1 1.2M17.7 15.3l2.1 1.2M4.2 16.5l2.1-1.2M17.7 8.7l2.1-1.2'),
  ),
  history: outline(p('M3.5 12a8.5 8.5 0 1 0 2.5-6M3.5 4.5V9H8M12 7.5V12l3 2')),

  // Actions
  plus: outline(p('M12 5v14M5 12h14')),
  x: outline(p('M6 6l12 12M18 6 6 18')),
  check: outline(p('m5 12.5 4.5 4.5L19 7.5')),
  'check-all': outline(p('m3 12.5 4 4L15 8M11 16.5l1 1L21 8')),
  refresh: outline(p('M20 12a8 8 0 1 1-2.34-5.66M20 4v5h-5')),
  pause: outline(p('M8 5.5v13M16 5.5v13')),
  play: outline(p('M7.5 5v14L19 12z')),
  download: outline(p('M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19.5h14')),
  external: outline(p('M14 4h6v6M20 4l-8.5 8.5M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5')),
  copy: outline(p('M9 9h11v11H9zM5 15H4V4h11v1')),
  edit: outline(p('M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4')),
  trash: outline(p('M4.5 6.5h15M9.5 6.5V4h5v2.5M6.5 6.5l1 13.5h9l1-13.5M10 10.5v6M14 10.5v6')),
  search: outline(c(10.5, 10.5, 6) + p('m15 15 5 5')),
  folder: outline(p('M3.5 6.5a1 1 0 0 1 1-1h5l2 2.5h8a1 1 0 0 1 1 1v9.5a1 1 0 0 1-1 1h-15a1 1 0 0 1-1-1z')),
  'chevron-right': outline(p('m9.5 6.5 5.5 5.5-5.5 5.5')),
  'chevron-left': outline(p('m14.5 6.5-5.5 5.5 5.5 5.5')),
  'chevron-down': outline(p('m6.5 9.5 5.5 5.5 5.5-5.5')),
  'arrow-left': outline(p('M19 12H5M11 6l-6 6 6 6')),
  'arrow-right': outline(p('M5 12h14M13 6l6 6-6 6')),
  more: outline(dot(5.5, 12) + dot(12, 12) + dot(18.5, 12)),
  bell: outline(p('M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0')),
  'bell-off': outline(p('M6 16.5V11c0-1 .3-2 .8-2.8M9 5.5A6 6 0 0 1 18 11v5.5l1.5 2H8M10 20.5a2 2 0 0 0 4 0M4 4l16 16')),
  terminal: outline(p('M4 5h16v14H4zM7.5 9.5l2.5 2.5-2.5 2.5M12.5 15h4')),
  link: outline(p('M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1')),
  upload: outline(p('M12 15V4M7.5 8.5 12 4l4.5 4.5M5 19.5h14')),
  sun: outline(c(12, 12, 4) + p('M12 2.5v2M12 19.5v2M4.6 4.6 6 6M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4')),
  moon: outline(p('M20 14.5A8.5 8.5 0 1 1 9.5 4a7 7 0 0 0 10.5 10.5z')),

  // Sources
  globe: outline(c(12, 12, 8.5) + p('M3.5 12h17M12 3.5c2.5 2.6 3.6 5.4 3.6 8.5s-1.1 5.9-3.6 8.5c-2.5-2.6-3.6-5.4-3.6-8.5S9.5 6.1 12 3.5z')),
  feed: outline(p('M5 5.5c7.2 0 13.5 6.3 13.5 13.5M5 11c4.1 0 8 3.9 8 8') + dot(6, 18)),
  branch: outline(c(6.5, 5.5, 2) + c(6.5, 18.5, 2) + c(17.5, 7.5, 2) + p('M6.5 7.5v9M17.5 9.5c0 4-5.5 3.5-10 7')),
  package: outline(p('M12 3 4 7v10l8 4 8-4V7zM4 7l8 4 8-4M12 11v10')),
  box: outline(p('M3.5 8.5h17v11h-17zM3.5 8.5 6 4.5h12l2.5 4M10 12.5h4')),
  braces: outline(p('M9 4.5c-2 0-3 1-3 3v2c0 1.2-.6 2-2 2.5 1.4.5 2 1.3 2 2.5v2c0 2 1 3 3 3M15 4.5c2 0 3 1 3 3v2c0 1.2.6 2 2 2.5-1.4.5-2 1.3-2 2.5v2c0 2-1 3-3 3')),
  shield: outline(p('M12 3 4.5 6v5.5c0 4.5 3.2 8 7.5 9.5 4.3-1.5 7.5-5 7.5-9.5V6z')),
  server: outline(p('M4 4h16v6H4zM4 14h16v6H4z') + dot(7.5, 7) + dot(7.5, 17)),
  sparkle: outline(p('M12 3.5 13.8 10.2 20.5 12 13.8 13.8 12 20.5 10.2 13.8 3.5 12 10.2 10.2z')),
  clock: outline(c(12, 12, 8.5) + p('M12 7.5V12l3 2')),
  alert: outline(p('M12 3.5 2.8 19.5h18.4zM12 10v4.2M12 17v.01')),
  info: outline(c(12, 12, 8.5) + p('M12 11v5M12 8v.01')),
  lock: outline(p('M5.5 11h13v9.5h-13zM8.5 11V7.5a3.5 3.5 0 0 1 7 0V11')),
  'cloud-off': outline(p('M7 18h10a4 4 0 0 0 1.4-7.75A6 6 0 0 0 7.2 8.6 4.5 4.5 0 0 0 7 18zM4 4l16 16')),
};

/** Registers {@link ICONS} at start-up. */
export function provideIcons() {
  return provideAppInitializer(() => {
    const registry = inject(MatIconRegistry);
    const sanitizer = inject(DomSanitizer);
    for (const [name, svg] of Object.entries(ICONS)) {
      // Our own constant markup, never data from a source or the network.
      registry.addSvgIconLiteralInNamespace('sp', name, sanitizer.bypassSecurityTrustHtml(svg));
    }
  });
}
