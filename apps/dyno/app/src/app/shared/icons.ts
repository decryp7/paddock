import { inject, provideAppInitializer } from '@angular/core';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { WIDGET_ICONS } from './widget-icons';

/** A 24 px line icon: 1.75 px strokes in `currentColor`, round caps and joins. */
const line = (body: string) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round">${body}</svg>`;

/**
 * Dyno's own icons, registered as literals in the `dy` namespace and used through `<dy-icon name="cpu">`. No icon
 * fonts and nothing fetched at run time: the CSP allows neither. Icons use `currentColor`, so they follow the theme.
 */
export const ICONS = {
  // Navigation.
  dashboard: line('<path d="M4 15a8 8 0 1 1 16 0"/><path d="M12 15l4-5"/><circle cx="12" cy="15" r="1.4"/><path d="M4 19h16"/>'),
  processes: line('<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M3 9h18M8 13h8M8 16.5h5"/>'),
  why: line('<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/><path d="M9.2 9.2a2 2 0 1 1 2.8 1.8c-.6.3-1 .8-1 1.5"/><path d="M11 15h.01"/>'),
  rules: line('<path d="M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1"/><circle cx="15" cy="6" r="2"/><circle cx="9" cy="12" r="2"/><circle cx="17" cy="18" r="2"/>'),
  health: line('<path d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z"/><path d="M7.5 12h2.5l1.2-2.2 1.8 4.2 1.2-2h2.3"/>'),
  reports: line('<path d="M4 20V10M10 20V4M16 20v-7M22 20H2"/>'),
  settings: line('<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z"/>'),
  sidebar: line('<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>'),

  // Metrics.
  cpu: line('<rect x="6" y="6" width="12" height="12" rx="1.5"/><rect x="9.5" y="9.5" width="5" height="5" rx=".5"/><path d="M9 2.5V6M15 2.5V6M9 18v3.5M15 18v3.5M2.5 9H6M2.5 15H6M18 9h3.5M18 15h3.5"/>'),
  memory: line('<rect x="2.5" y="7" width="19" height="9" rx="1.5"/><path d="M6 10v3M10 10v3M14 10v3M18 10v3M5 16v2.5M19 16v2.5M9 16v2.5M15 16v2.5"/>'),
  disk: line('<rect x="3" y="4" width="18" height="16" rx="2"/><circle cx="12" cy="11" r="4"/><path d="M12 11h.01M7 17h.01"/>'),
  network: line('<path d="M5 12.5a10 10 0 0 1 14 0M8.5 16a5 5 0 0 1 7 0M2 9a14.5 14.5 0 0 1 20 0"/><circle cx="12" cy="19.2" r=".9" fill="currentColor"/>'),
  gpu: line('<rect x="2.5" y="6" width="19" height="11" rx="1.5"/><circle cx="9" cy="11.5" r="2.8"/><path d="M15 9.5h3.5M15 13.5h3.5M5 17v2.5M9 17v2.5"/>'),
  battery: line('<rect x="2.5" y="7" width="17" height="10" rx="2"/><path d="M22 10.5v3"/><path d="M6 10v4M9.5 10v4M13 10v4"/>'),
  temperature: line('<path d="M14 14.8V5a2 2 0 1 0-4 0v9.8a4 4 0 1 0 4 0z"/><path d="M12 9v7"/>'),
  fan: line('<circle cx="12" cy="12" r="1.6"/><path d="M12 10.4C11 7 11.5 3.5 14 3.5c2 0 2.5 3-2 6.9zM13.6 12c3.4-1 6.9-.5 6.9 2 0 2-3 2.5-6.9-2zM12 13.6c1 3.4.5 6.9-2 6.9-2 0-2.5-3 2-6.9zM10.4 12C7 13 3.5 12.5 3.5 10c0-2 3-2.5 6.9 2z"/>'),
  plug: line('<path d="M9 2.5V7M15 2.5V7M6.5 7h11v3.5a5.5 5.5 0 0 1-11 0zM12 16v5.5"/>'),
  bolt: line('<path d="M13 2.5 4.5 13.5H12L11 21.5l8.5-11H12z"/>'),
  clock: line('<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>'),
  power: line('<path d="M12 3v8"/><path d="M6.3 6.8a8 8 0 1 0 11.4 0"/>'),

  // Actions.
  search: line('<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>'),
  play: line('<path d="M7 4.5v15l12-7.5z"/>'),
  pause: line('<path d="M8 5v14M16 5v14"/>'),
  stop: line('<rect x="5.5" y="5.5" width="13" height="13" rx="2"/>'),
  close: line('<path d="M6 6l12 12M18 6 6 18"/>'),
  check: line('<path d="m4.5 12.5 5 5 10-11"/>'),
  more: line('<circle cx="5.5" cy="12" r="1" fill="currentColor"/><circle cx="12" cy="12" r="1" fill="currentColor"/><circle cx="18.5" cy="12" r="1" fill="currentColor"/>'),
  'chevron-down': line('<path d="m6 9 6 6 6-6"/>'),
  'chevron-right': line('<path d="m9 6 6 6-6 6"/>'),
  'chevron-left': line('<path d="m15 6-6 6 6 6"/>'),
  'arrow-up': line('<path d="M12 19V5M6 11l6-6 6 6"/>'),
  'arrow-down': line('<path d="M12 5v14M6 13l6 6 6-6"/>'),
  'arrow-right': line('<path d="M5 12h14M13 6l6 6-6 6"/>'),
  undo: line('<path d="M9 14 4 9l5-5"/><path d="M4 9h10.5a5.5 5.5 0 0 1 0 11H11"/>'),
  refresh: line('<path d="M20 12a8 8 0 1 1-2.3-5.7"/><path d="M20 4v5h-5"/>'),
  folder: line('<path d="M3 7a2 2 0 0 1 2-2h4l2 2.5h8a2 2 0 0 1 2 2V18a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>'),
  file: line('<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>'),
  trash: line('<path d="M4 7h16M10 11v6M14 11v6"/><path d="M6 7l1 12.5a1.5 1.5 0 0 0 1.5 1.5h7a1.5 1.5 0 0 0 1.5-1.5L18 7"/><path d="M9 7V4.5h6V7"/>'),
  restore: line('<path d="M3.5 12a8.5 8.5 0 1 0 2.5-6"/><path d="M3.5 4v4.5H8"/><path d="M12 8v4l2.5 1.5"/>'),
  camera: line('<path d="M4 8a2 2 0 0 1 2-2h1.5L9 4h6l1.5 2H18a2 2 0 0 1 2 2v9a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2z"/><circle cx="12" cy="12.5" r="3.5"/>'),
  diff: line('<path d="M12 3v18"/><path d="M4 9h5M6.5 6.5v5M15 9h5M4 16h5M15 16h5"/>'),
  tree: line('<rect x="3" y="3" width="7" height="5" rx="1"/><rect x="14" y="10" width="7" height="5" rx="1"/><rect x="14" y="17" width="7" height="4.5" rx="1"/><path d="M6.5 8v11H14M6.5 12.5H14"/>'),
  group: line('<rect x="3" y="4" width="18" height="5" rx="1.5"/><path d="M7 12.5h14M7 16.5h14M7 20.5h10"/><path d="M3.5 12.5v8"/>'),
  list: line('<path d="M8 6h13M8 12h13M8 18h13"/><path d="M3.5 6h.01M3.5 12h.01M3.5 18h.01"/>'),
  filter: line('<path d="M3 5h18l-7 8.5V19l-4 2v-7.5z"/>'),
  sort: line('<path d="M7 4v16M3.5 16.5 7 20l3.5-3.5M17 20V4M13.5 7.5 17 4l3.5 3.5"/>'),
  lock: line('<rect x="5" y="10.5" width="14" height="10" rx="2"/><path d="M8 10.5V7a4 4 0 0 1 8 0v3.5"/>'),
  shield: line('<path d="M12 3 4.5 6v5.5c0 4.6 3.2 8.4 7.5 9.5 4.3-1.1 7.5-4.9 7.5-9.5V6z"/>'),
  'shield-check': line('<path d="M12 3 4.5 6v5.5c0 4.6 3.2 8.4 7.5 9.5 4.3-1.1 7.5-4.9 7.5-9.5V6z"/><path d="m8.8 12 2.2 2.2 4.2-4.4"/>'),
  leaf: line('<path d="M5 19c0-8 5-13.5 14.5-14.5C19 14 13.5 19 5 19z"/><path d="M5 19c3-4 6-6.5 9.5-8.5"/>'),
  gauge: line('<path d="M3.5 16a8.5 8.5 0 1 1 17 0"/><path d="M12 16 8.5 10.5"/><path d="M6.5 16h11"/>'),
  sparkle: line('<path d="M12 3c.6 4.4 2.1 6.4 6.5 7-4.4.6-5.9 2.6-6.5 7-.6-4.4-2.1-6.4-6.5-7 4.4-.6 5.9-2.6 6.5-7z"/><path d="M19 15.5c.3 1.9.9 2.7 2.5 3-1.6.3-2.2 1.1-2.5 3-.3-1.9-.9-2.7-2.5-3 1.6-.3 2.2-1.1 2.5-3z"/>'),
  flame: line('<path d="M12 21.5a6.5 6.5 0 0 0 6.5-6.5c0-4-3-6-3.5-10-2.5 1.5-4 4-4 6.5-1-1-1.5-2.5-1.5-3.5-2 2-4 4.5-4 7a6.5 6.5 0 0 0 6.5 6.5z"/>'),
  alert: line('<path d="M10.3 4.2 2.6 17.5A2 2 0 0 0 4.3 20.5h15.4a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z"/><path d="M12 9.5v4.5M12 17.2h.01"/>'),
  info: line('<circle cx="12" cy="12" r="9"/><path d="M12 11v5.5M12 7.8h.01"/>'),
  bell: line('<path d="M6 16V11a6 6 0 0 1 12 0v5l1.5 2h-15z"/><path d="M10 20.5a2 2 0 0 0 4 0"/>'),
  external: line('<path d="M14 4h6v6M20 4l-9 9"/><path d="M18 14v4a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h4"/>'),
  command: line('<path d="M9 6a3 3 0 1 0-3 3h12a3 3 0 1 0-3-3v12a3 3 0 1 0 3-3H6a3 3 0 1 0 3 3z"/>'),
  sun: line('<circle cx="12" cy="12" r="4"/><path d="M12 2.5v2M12 19.5v2M2.5 12h2M19.5 12h2M5.3 5.3l1.4 1.4M17.3 17.3l1.4 1.4M5.3 18.7l1.4-1.4M17.3 6.7l1.4-1.4"/>'),
  moon: line('<path d="M20 14.5A8 8 0 1 1 9.5 4a6.5 6.5 0 0 0 10.5 10.5z"/>'),
  monitor: line('<rect x="2.5" y="4" width="19" height="13" rx="2"/><path d="M8.5 21h7M12 17v4"/>'),
  puzzle: line('<path d="M10 4.5a2 2 0 1 1 4 0V6h4v4h-1.5a2 2 0 1 0 0 4H18v4h-4v-1.5a2 2 0 1 0-4 0V18H6v-4h1.5a2 2 0 1 0 0-4H6V6h4z"/>'),
  eye: line('<path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z"/><circle cx="12" cy="12" r="3"/>'),
  copy: line('<rect x="8.5" y="8.5" width="12" height="12" rx="2"/><path d="M15.5 8.5V5.5a2 2 0 0 0-2-2h-8a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h3"/>'),
  plus: line('<path d="M12 5v14M5 12h14"/>'),
  minus: line('<path d="M5 12h14"/>'),
  edit: line('<path d="M4 20h4L19 9a2.8 2.8 0 0 0-4-4L4 16z"/><path d="m13.5 6.5 4 4"/>'),
  user: line('<circle cx="12" cy="8" r="4"/><path d="M4.5 20.5a7.5 7.5 0 0 1 15 0"/>'),
  calendar: line('<rect x="3.5" y="5" width="17" height="15.5" rx="2"/><path d="M3.5 10h17M8 3v4M16 3v4"/>'),
  pin: line('<path d="M9 4h6l-.8 5.5 3 3.2H6.8l3-3.2z"/><path d="M12 12.7V20"/>'),
  trend: line('<path d="M3 17l6-6 4 4 8-8"/><path d="M15 7h6v6"/>'),
  globe: line('<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18"/>'),
  layers: line('<path d="m12 3 9 5-9 5-9-5z"/><path d="m3 13 9 5 9-5"/>'),
  target: line('<circle cx="12" cy="12" r="8.5"/><circle cx="12" cy="12" r="4.5"/><circle cx="12" cy="12" r=".8" fill="currentColor"/>'),

  // Profiles.
} as const;

export type IconName = keyof typeof ICONS;

/** Registers {@link ICONS} at start-up. */
export function provideIcons() {
  return provideAppInitializer(() => {
    const registry = inject(MatIconRegistry);
    const sanitizer = inject(DomSanitizer);
    for (const [name, svg] of Object.entries(ICONS)) {
      // Our own constant markup, never data from the engine or the network.
      registry.addSvgIconLiteralInNamespace('dy', name, sanitizer.bypassSecurityTrustHtml(svg));
    }
    // The widget catalogues' icon names (`KindInfo.icon`) are bare, so they live in the default namespace.
    for (const [name, svg] of Object.entries(WIDGET_ICONS)) registry.addSvgIconLiteral(name, sanitizer.bypassSecurityTrustHtml(svg));
  });
}
