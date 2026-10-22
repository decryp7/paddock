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

/**
 * Pitcrew's own icons, registered as literals in the `pc` namespace (`<mat-icon svgIcon="pc:refresh">`). No icon
 * fonts and nothing fetched at run time: the CSP allows neither.
 */
export const ICONS: Record<string, string> = {
  // Navigation
  dashboard: outline(p('M4 4h7v9H4zM13 4h7v5h-7zM13 11h7v9h-7zM4 15h7v5H4z')),
  runner: outline(c(12, 12, 8.5) + p('M10.2 8.6v6.8l5.6-3.4z')),
  activity: outline(p('M3 12h4l2.5-6 5 12 2.5-6h4')),
  logs: outline(p('M4 5h16v14H4zM7.5 9.5l2.5 2.5-2.5 2.5M12.5 15h4')),
  updates: outline(c(12, 12, 8.5) + p('M12 16V8M8.5 11.5 12 8l3.5 3.5')),
  settings: outline(
    c(12, 12, 3) +
      p('M12 2.8v2.4M12 18.8v2.4M4.2 7.5l2.1 1.2M17.7 15.3l2.1 1.2M4.2 16.5l2.1-1.2M17.7 8.7l2.1-1.2') +
      p('M12 5.2a6.8 6.8 0 1 0 0 13.6 6.8 6.8 0 0 0 0-13.6z'),
  ),

  // Status
  'check-circle': outline(c(12, 12, 8.5) + p('m8.5 12.2 2.4 2.4 4.6-4.9')),
  'play-circle': outline(c(12, 12, 8.5) + p('M10.2 8.8v6.4l5.2-3.2z')),
  'pause-circle': outline(c(12, 12, 8.5) + p('M10 9v6M14 9v6')),
  'stop-circle': outline(c(12, 12, 8.5) + p('M9.5 9.5h5v5h-5z')),
  'x-octagon': outline(p('M8.3 3h7.4L21 8.3v7.4L15.7 21H8.3L3 15.7V8.3zM9.5 9.5l5 5M14.5 9.5l-5 5')),
  alert: outline(p('M12 3.5 2.8 19.5h18.4zM12 10v4.2M12 17v.01')),
  info: outline(c(12, 12, 8.5) + p('M12 11v5M12 8v.01')),
  loader: outline(p('M12 3a9 9 0 1 0 9 9')),
  hourglass: outline(p('M7 3h10M7 21h10M8 3c0 4.5 8 4.5 8 9s-8 4.5-8 9M16 3c0 4.5-8 4.5-8 9s8 4.5 8 9')),
  'link-off': outline(p('M9.5 14.5 14.5 9.5M10 6.5l1.2-1.2a4 4 0 0 1 5.6 5.6L15.5 12M14 17.5l-1.2 1.2a4 4 0 0 1-5.6-5.6L8.5 12M4 4l16 16')),
  'cloud-off': outline(p('M7 18h10a4 4 0 0 0 1.4-7.75A6 6 0 0 0 7.2 8.6 4.5 4.5 0 0 0 7 18zM4 4l16 16')),
  user: outline(c(12, 8, 3.5) + p('M5 20a7 7 0 0 1 14 0')),
  clock: outline(c(12, 12, 8.5) + p('M12 7.5V12l3 2')),

  // Actions
  play: outline(p('M7.5 5v14L19 12z')),
  stop: outline(p('M6.5 6.5h11v11h-11z')),
  pause: outline(p('M8 5.5v13M16 5.5v13')),
  refresh: outline(p('M20 12a8 8 0 1 1-2.34-5.66M20 4v5h-5')),
  download: outline(p('M12 4v11M7.5 10.5 12 15l4.5-4.5M5 19.5h14')),
  undo: outline(p('M9 14 4.5 9.5 9 5M4.5 9.5H15a5 5 0 0 1 0 10h-3')),
  broom: outline(p('M14.5 3.5 11 10M7 10.5h8.5l1.5 4.5-1 5.5H6l-.5-5.5zM9.5 15.5v5M13 15.5v5')),
  plus: outline(p('M12 5v14M5 12h14')),
  x: outline(p('M6 6l12 12M18 6 6 18')),
  check: outline(p('m5 12.5 4.5 4.5L19 7.5')),
  more: outline('<circle cx="5.5" cy="12" r="1.1" fill="currentColor"/><circle cx="12" cy="12" r="1.1" fill="currentColor"/><circle cx="18.5" cy="12" r="1.1" fill="currentColor"/>'),
  'chevron-down': outline(p('m6.5 9.5 5.5 5.5 5.5-5.5')),
  'chevron-right': outline(p('m9.5 6.5 5.5 5.5-5.5 5.5')),
  'chevron-left': outline(p('m14.5 6.5-5.5 5.5 5.5 5.5')),
  external: outline(p('M14 4h6v6M20 4l-8.5 8.5M18 14v5a1 1 0 0 1-1 1H5a1 1 0 0 1-1-1V7a1 1 0 0 1 1-1h5')),
  copy: outline(p('M9 9h11v11H9zM5 15H4V4h11v1')),
  search: outline(c(10.5, 10.5, 6) + p('m15 15 5 5')),
  follow: outline(p('M12 4v12M7 11.5l5 5 5-5M5 20h14')),
  trash: outline(p('M4.5 6.5h15M9.5 6.5V4h5v2.5M6.5 6.5l1 13.5h9l1-13.5M10 10.5v6M14 10.5v6')),
  pin: outline(p('M9 3.5h6M10 3.5v6.2L6.5 14h11L14 9.7V3.5M12 14v6.5')),
  skip: outline(p('M6 5.5v13l9-6.5zM18 5.5v13')),
  power: outline(p('M12 3v8M7 6.5a7 7 0 1 0 10 0')),
  filter: outline(p('M4 5h16l-6 7.5V19l-4 1.5v-8z')),

  // Things
  cpu: outline(p('M7 7h10v10H7zM10 10h4v4h-4zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4')),
  memory: outline(p('M3.5 7.5h17v9h-17zM7 7.5v9M10.5 7.5v9M14 7.5v9M17.5 7.5v9M6 16.5V19M18 16.5V19')),
  disk: outline(p('M3.5 13.5 6 5h12l2.5 8.5v5h-17zM3.5 13.5h17') + '<circle cx="16.5" cy="16" r="0.9" fill="currentColor"/>'),
  server: outline(p('M4 4h16v6H4zM4 14h16v6H4z') + '<circle cx="7.5" cy="7" r="0.9" fill="currentColor"/><circle cx="7.5" cy="17" r="0.9" fill="currentColor"/>'),
  shield: outline(p('M12 3 4.5 6v5.5c0 4.5 3.2 8 7.5 9.5 4.3-1.5 7.5-5 7.5-9.5V6z')),
  key: outline(c(8, 15, 4) + p('m11 12 8.5-8.5M16 6.5l2.5 2.5M14 8.5l2 2')),
  lock: outline(p('M5.5 11h13v9.5h-13zM8.5 11V7.5a3.5 3.5 0 0 1 7 0V11')),
  sun: outline(c(12, 12, 4) + p('M12 2.5v2M12 19.5v2M4.6 4.6 6 6M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4')),
  moon: outline(p('M20 14.5A8.5 8.5 0 1 1 9.5 4a7 7 0 0 0 10.5 10.5z')),
  monitor: outline(p('M3.5 4.5h17v11h-17zM8.5 20h7M12 15.5V20')),
  folder: outline(p('M3.5 6.5a1 1 0 0 1 1-1h5l2 2.5h8a1 1 0 0 1 1 1v9.5a1 1 0 0 1-1 1h-15a1 1 0 0 1-1-1z')),
  tag: outline(p('M3.5 12.5V4.5a1 1 0 0 1 1-1h8l8 8-9 9z') + '<circle cx="8" cy="8" r="1.2" fill="currentColor"/>'),
  branch: outline(c(6.5, 5.5, 2) + c(6.5, 18.5, 2) + c(17.5, 7.5, 2) + p('M6.5 7.5v9M17.5 9.5c0 4-5.5 3.5-10 7')),
  bell: outline(p('M6 16.5V11a6 6 0 0 1 12 0v5.5l1.5 2h-15zM10 20.5a2 2 0 0 0 4 0')),
  gauge: outline(p('M4 17a8.5 8.5 0 1 1 16 0M12 13l4-4') + c(12, 13.5, 1.2)),
  sparkle: outline(p('M12 3.5 13.8 10.2 20.5 12 13.8 13.8 12 20.5 10.2 13.8 3.5 12 10.2 10.2z')),
  calendar: outline(p('M4 6h16v14H4zM4 10h16M8.5 3.5v4M15.5 3.5v4')),
  package: outline(p('M12 3 4 7v10l8 4 8-4V7zM4 7l8 4 8-4M12 11v10')),
  'file-text': outline(p('M6 3.5h8.5L19 8v12.5H6zM14 3.5V8h5M9 12.5h7M9 16h7')),
  code: outline(p('M8.5 7 3.5 12l5 5M15.5 7l5 5-5 5M13.5 5l-3 14')),
  'chevron-up': outline(p('M6 15l6-6 6 6')),
  minus: outline(p('M5 12h14')),
  pitcrew: outline(p('M12 3.5a8.5 8.5 0 1 0 8.5 8.5') + p('M12 12l5-5M16 3.5h4.5V8') + c(12, 12, 1.4)),
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
