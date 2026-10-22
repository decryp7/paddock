import { inject, provideAppInitializer } from '@angular/core';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';

/** A 24×24 outline icon from path data, stroked in `currentColor` so it follows the theme. */
const s = (body: string) =>
  `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;

/**
 * Pitwall's own icons, registered as literals in the `pw` namespace (`<mat-icon svgIcon="pw:models">`). No icon fonts
 * and nothing fetched at run time: the CSP allows neither.
 */
export const ICONS: Record<string, string> = {
  // Navigation
  dashboard: s('<rect x="3" y="3" width="7.5" height="9" rx="1.5"/><rect x="13.5" y="3" width="7.5" height="5" rx="1.5"/><rect x="13.5" y="11" width="7.5" height="10" rx="1.5"/><rect x="3" y="15" width="7.5" height="6" rx="1.5"/>'),
  models: s('<path d="M12 2.8 20 7.4v9.2l-8 4.6-8-4.6V7.4z"/><path d="m4 7.4 8 4.6 8-4.6M12 12v9.2"/>'),
  engines: s('<rect x="5" y="5" width="14" height="14" rx="2.5"/><rect x="9" y="9" width="6" height="6" rx="1"/><path d="M9 2v3M15 2v3M9 19v3M15 19v3M2 9h3M2 15h3M19 9h3M19 15h3"/>'),
  apps: s('<circle cx="8" cy="15" r="4.2"/><path d="m11 12 8.5-8.5M16.5 6.5l2.5 2.5M14 9l2 2"/>'),
  requests: s('<path d="M4 6h16M4 12h16M4 18h10"/><circle cx="19" cy="18" r="1.6"/>'),
  playground: s('<path d="M20 14.5a2.5 2.5 0 0 1-2.5 2.5H8.5L4 20.5V6.5A2.5 2.5 0 0 1 6.5 4h11A2.5 2.5 0 0 1 20 6.5z"/><path d="M8.5 9.5h7M8.5 12.5h4.5"/>'),
  benchmarks: s('<path d="M4 18a8 8 0 1 1 16 0"/><path d="m12 18 4-6"/><path d="M4 18h16"/>'),
  settings: s('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>'),

  // Actions
  refresh: s('<path d="M20 12a8 8 0 1 1-2.34-5.66"/><path d="M20 4v5h-5"/>'),
  pause: s('<rect x="6.5" y="5" width="3.5" height="14" rx="1"/><rect x="14" y="5" width="3.5" height="14" rx="1"/>'),
  play: s('<path d="M7 4.8v14.4a.8.8 0 0 0 1.2.7l11.5-7.2a.8.8 0 0 0 0-1.4L8.2 4.1A.8.8 0 0 0 7 4.8z"/>'),
  stop: s('<rect x="6" y="6" width="12" height="12" rx="2"/>'),
  pin: s('<path d="M9 3h6l-1 6 4 3v2H6v-2l4-3z"/><path d="M12 14v7"/>'),
  unpin: s('<path d="M9 3h6l-1 6 4 3v2H6v-2l4-3z"/><path d="M12 14v7M3 3l18 18"/>'),
  trash: s('<path d="M4 7h16M10 11v6M14 11v6M5.5 7l1 12a2 2 0 0 0 2 2h7a2 2 0 0 0 2-2l1-12M9 7V4.5A1.5 1.5 0 0 1 10.5 3h3A1.5 1.5 0 0 1 15 4.5V7"/>'),
  download: s('<path d="M12 4v11M7 10.5l5 5 5-5M4.5 20h15"/>'),
  search: s('<circle cx="11" cy="11" r="6.5"/><path d="m20 20-4.4-4.4"/>'),
  copy: s('<rect x="8.5" y="8.5" width="12" height="12" rx="2"/><path d="M15.5 8.5V5.5a2 2 0 0 0-2-2h-8a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h3"/>'),
  plus: s('<path d="M12 5v14M5 12h14"/>'),
  close: s('<path d="M6 6l12 12M18 6 6 18"/>'),
  check: s('<path d="m5 12.5 4.5 4.5L19 7.5"/>'),
  more: s('<circle cx="5.5" cy="12" r="1.1"/><circle cx="12" cy="12" r="1.1"/><circle cx="18.5" cy="12" r="1.1"/>'),
  'chevron-right': s('<path d="m9.5 6 6 6-6 6"/>'),
  'chevron-down': s('<path d="m6 9.5 6 6 6-6"/>'),
  'arrow-left': s('<path d="M19 12H5M11 6l-6 6 6 6"/>'),
  'arrow-up-right': s('<path d="M7 17 17 7M8 7h9v9"/>'),
  send: s('<path d="M5 12h13M12.5 6 18.5 12l-6 6"/>'),
  broom: s('<path d="m14.5 3.5-5 9M6 12.5l6.5 3.5-2 5.5c-3-1-6-3-7.5-6z"/><path d="m9.5 12.5 3 1.6"/>'),
  release: s('<path d="M12 3v8M7.5 7.5 12 3l4.5 4.5"/><path d="M5 13.5v4A2.5 2.5 0 0 0 7.5 20h9a2.5 2.5 0 0 0 2.5-2.5v-4"/>'),
  rollback: s('<path d="M4 12a8 8 0 1 0 2.34-5.66"/><path d="M4 4v5h5"/>'),
  open: s('<path d="M14 4h6v6M20 4l-8.5 8.5M18 14.5V18a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h3.5"/>'),
  compare: s('<rect x="3" y="4" width="7.5" height="16" rx="1.5"/><rect x="13.5" y="4" width="7.5" height="16" rx="1.5"/>'),
  sliders: s('<path d="M4 7h10M18 7h2M4 17h2M10 17h10"/><circle cx="16" cy="7" r="2"/><circle cx="8" cy="17" r="2"/>'),

  // Things
  memory: s('<rect x="3" y="7" width="18" height="10" rx="1.5"/><path d="M7 7v10M11 7v10M15 7v10M5 17v3M19 17v3M9 17v3M13 17v3M17 17v3"/>'),
  cpu: s('<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M9.5 2.5v3.5M14.5 2.5v3.5M9.5 18v3.5M14.5 18v3.5M2.5 9.5H6M2.5 14.5H6M18 9.5h3.5M18 14.5h3.5"/>'),
  gpu: s('<rect x="2.5" y="6" width="19" height="11" rx="2"/><circle cx="9" cy="11.5" r="2.8"/><path d="M15 9.5h3.5M15 13.5h3.5M5 17v2.5M9 17v2.5"/>'),
  bolt: s('<path d="M13 2.5 4.5 13.5H12l-1 8 8.5-11H12z"/>'),
  clock: s('<circle cx="12" cy="12" r="8.5"/><path d="M12 7.5V12l3 2"/>'),
  queue: s('<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 12.5 9 5 9-5"/><path d="m3 17 9 5 9-5" opacity=".55"/>'),
  key: s('<circle cx="8" cy="15" r="4.2"/><path d="m11 12 8.5-8.5M16.5 6.5l2.5 2.5M14 9l2 2"/>'),
  shield: s('<path d="M12 3 4.5 6v5.5c0 4.6 3.2 8.4 7.5 9.5 4.3-1.1 7.5-4.9 7.5-9.5V6z"/><path d="m9 12 2.2 2.2L15.5 10"/>'),
  lock: s('<rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V7.5a4 4 0 0 1 8 0v3"/>'),
  eye: s('<path d="M2.5 12S6 5 12 5s9.5 7 9.5 7-3.5 7-9.5 7-9.5-7-9.5-7z"/><circle cx="12" cy="12" r="2.8"/>'),
  'eye-off': s('<path d="M9.9 5.2A9.4 9.4 0 0 1 12 5c6 0 9.5 7 9.5 7a16 16 0 0 1-2.4 3.3M6.6 6.6A16 16 0 0 0 2.5 12s3.5 7 9.5 7a9.2 9.2 0 0 0 5.4-1.6M3 3l18 18M9.9 9.9a2.8 2.8 0 0 0 4.2 4.2"/>'),
  sun: s('<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M4.6 4.6 6 6M18 18l1.4 1.4M2.5 12h2M19.5 12h2M4.6 19.4 6 18M18 6l1.4-1.4"/>'),
  moon: s('<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z"/>'),
  monitor: s('<rect x="3" y="4" width="18" height="12.5" rx="2"/><path d="M8.5 20.5h7M12 16.5v4"/>'),
  globe: s('<circle cx="12" cy="12" r="8.5"/><path d="M3.5 12h17M12 3.5c2.5 2.6 3.7 5.4 3.7 8.5S14.5 17.9 12 20.5C9.5 17.9 8.3 15.1 8.3 12S9.5 6.1 12 3.5z"/>'),
  offline: s('<path d="M3 3l18 18M8.5 8.6A10.5 10.5 0 0 0 3.5 11M5.5 14.5a7.5 7.5 0 0 1 4.2-2.3M12.8 7.5a10.5 10.5 0 0 1 7.7 3.5M16.2 12.4a7.5 7.5 0 0 1 2.3 2.1M9 17.5a4 4 0 0 1 6 0M12 20.5h.01"/>'),
  info: s('<circle cx="12" cy="12" r="8.5"/><path d="M12 11v5.5M12 7.8v.2"/>'),
  alert: s('<path d="M10.3 4 2.6 17.5A2 2 0 0 0 4.3 20.5h15.4a2 2 0 0 0 1.7-3L13.7 4a2 2 0 0 0-3.4 0z"/><path d="M12 9.5v4.5M12 17v.2"/>'),
  error: s('<circle cx="12" cy="12" r="8.5"/><path d="m9 9 6 6M15 9l-6 6"/>'),
  success: s('<circle cx="12" cy="12" r="8.5"/><path d="m8.5 12.2 2.4 2.4 4.6-4.8"/>'),
  sparkles: s('<path d="M11 3.5 12.6 8 17 9.5l-4.4 1.6L11 15.5l-1.6-4.4L5 9.5 9.4 8z"/><path d="M18 14.5l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/>'),
  power: s('<path d="M12 3v8.5"/><path d="M6.6 6.6a8 8 0 1 0 10.8 0"/>'),
  server: s('<rect x="3.5" y="3.5" width="17" height="7" rx="1.5"/><rect x="3.5" y="13.5" width="17" height="7" rx="1.5"/><path d="M7.5 7h.01M7.5 17h.01"/>'),
  disk: s('<ellipse cx="12" cy="6" rx="8" ry="3"/><path d="M4 6v12c0 1.7 3.6 3 8 3s8-1.3 8-3V6"/><path d="M4 12c0 1.7 3.6 3 8 3s8-1.3 8-3"/>'),
  link: s('<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>'),
  tool: s('<path d="M14.5 6.5a4 4 0 0 0 5 5L21 13l-8 8-3.5-3.5 1.5-1.5a4 4 0 0 1-5-5L3 8l3-3 3 3a4 4 0 0 1 5.5-1.5z"/>'),
  heart: s('<path d="M12 20s-7.5-4.6-7.5-10A4.3 4.3 0 0 1 12 7.4 4.3 4.3 0 0 1 19.5 10c0 5.4-7.5 10-7.5 10z"/>'),
  vision: s('<path d="M2.5 12S6 5 12 5s9.5 7 9.5 7-3.5 7-9.5 7-9.5-7-9.5-7z"/><circle cx="12" cy="12" r="2.8"/>'),
  braces: s('<path d="M8.5 4C6.5 4 6 5 6 6.5v2.2C6 10.2 5 11.5 3.5 12c1.5.5 2.5 1.8 2.5 3.3v2.2C6 19 6.5 20 8.5 20M15.5 4c2 0 2.5 1 2.5 2.5v2.2c0 1.5 1 2.8 2.5 3.3-1.5.5-2.5 1.8-2.5 3.3v2.2c0 1.5-.5 2.5-2.5 2.5"/>'),
  vector: s('<circle cx="12" cy="5.5" r="2"/><circle cx="5.5" cy="17.5" r="2"/><circle cx="18.5" cy="17.5" r="2"/><path d="m11 7.3-4.6 8.4M13 7.3l4.6 8.4M7.5 17.5h9"/>'),
  sort: s('<path d="M4 6h16M4 12h11M4 18h6"/>'),
  activity: s('<path d="M3 12h4l2.5-6 5 12 2.5-6h4"/>'),
  // The Pitwall brand mark (suite-icons/pitwall/small/pitwall-32.svg): Enamel purple tile, white radio bubble.
  logo: '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><defs><linearGradient id="pwLogoBg" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="#7B46DB"/><stop offset="1" stop-color="#43198F"/></linearGradient><mask id="pwLogoMask" maskUnits="userSpaceOnUse" x="-2000" y="-2000" width="5000" height="5000"><rect x="-2000" y="-2000" width="5000" height="5000" fill="#fff"/><path d="M11.5 13.5V16.5M14 12V18M16.5 11V19M19 12V18M21.5 13.5V16.5" fill="none" stroke="#000" stroke-width="1.5" stroke-linecap="round"/></mask></defs><path d="M8 1H24A7 7 0 0 1 31 8V24A7 7 0 0 1 24 31H8A7 7 0 0 1 1 24V8A7 7 0 0 1 8 1Z" fill="url(#pwLogoBg)"/><path d="M8 1.25H24A6.75 6.75 0 0 1 30.75 8V24A6.75 6.75 0 0 1 24 30.75H8A6.75 6.75 0 0 1 1.25 24V8A6.75 6.75 0 0 1 8 1.25Z" fill="none" stroke="#24094F" stroke-opacity="0.35" stroke-width="0.5"/><path d="M11 7.5H21A5 5 0 0 1 26 12.5V17.5A5 5 0 0 1 21 22.5H11A5 5 0 0 1 6 17.5V12.5A5 5 0 0 1 11 7.5ZM8.5 20L14 22L8 26Z" fill="#FFFFFF" stroke="#FFFFFF" stroke-width="1.2" stroke-linejoin="round" mask="url(#pwLogoMask)"/></svg>',
};

/** Registers {@link ICONS} with Material's icon registry. */
export function registerIcons(registry: MatIconRegistry, sanitizer: DomSanitizer): void {
  for (const [name, svg] of Object.entries(ICONS)) {
    // Our own constant markup, never data from an app, a model or the network.
    registry.addSvgIconLiteralInNamespace('pw', name, sanitizer.bypassSecurityTrustHtml(svg));
  }
}

/** Registers {@link ICONS} at start-up. */
export function provideIcons() {
  return provideAppInitializer(() => registerIcons(inject(MatIconRegistry), inject(DomSanitizer)));
}
