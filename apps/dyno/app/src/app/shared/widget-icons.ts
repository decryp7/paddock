import { outlineIconWithDots, type OutlineDot } from '@decryp7/paddock-ui/icons';

/**
 * The icon names Dyno's widget catalogues use (`KindInfo.icon`, set in `dyno-core/src/widgets/{popover,dashboard,menu_bar}.rs`),
 * drawn with the family's outline helper and registered under those bare names for `<mat-icon svgIcon="alarm">` (the
 * chooser, the widget frame). A name the engine adds needs its glyph here; the spec checks every name is registered.
 */
const glyph = (paths: string[], dots: OutlineDot[] = []): string => outlineIconWithDots(paths, dots);

export const WIDGET_ICONS: Readonly<Record<string, string>> = {
  alarm: glyph(['M6 10.5a6 6 0 0 1 12 0V16l1.5 2h-15L6 16z', 'M10 20.5a2 2 0 0 0 4 0']),
  disk: glyph(['M3.5 14.5h17v5h-17z', 'M5.5 14.5l2-9h9l2 9'], [[17, 17, 0.9]]),
  cpu: glyph(['M7 7h10v10H7z', 'M10 10h4v4h-4z', 'M9 3.5V7M15 3.5V7M9 17v3.5M15 17v3.5M3.5 9H7M3.5 15H7M17 9h3.5M17 15h3.5']),
  network: glyph(['M4 9.5a11 11 0 0 1 16 0', 'M7 13a6.5 6.5 0 0 1 10 0', 'M10 16.5a2.2 2.2 0 0 1 4 0'], [[12, 19.5, 1]]),
  backup: glyph(['M12 3 19.5 6v5.5c0 4.6-3.1 8-7.5 9.5-4.4-1.5-7.5-4.9-7.5-9.5V6z', 'M9 12l2.2 2.2L15.5 10']),
  issue: glyph(['M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13Z', 'M15.25 15.25 20 20', 'M10.5 8v3']),
  counters: glyph(['M9 4h6l-1 5.5 3 3H7l3-3z', 'M12 12.5V20']),
  apps: glyph(['M4 5h16v14H4z', 'M4 9.5h16M8 13h8M8 16h5']),
  graph: glyph(['M3 17l5-5 4 4 8-8', 'M15 8h5v5']),
  number: glyph(['M6 8v8M6 8 4 10', 'M11 10a2 2 0 1 1 4 0c0 2-4 3-4 6h4']),
  bars: glyph(['M5 19v-8M12 19V5M19 19v-6']),
  ring: glyph(['M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17Z', 'M12 7.5V12h4.5']),
  heatmap: glyph(['M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z']),
  health: glyph(['M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z']),
  diagnosis: glyph(['M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13Z', 'M15.25 15.25 20 20', 'M8.5 10.5h4']),
  events: glyph(['M8 6h12M8 12h12M8 18h12'], [[4, 6, 0.9], [4, 12, 0.9], [4, 18, 0.9]]),
  history: glyph(['M4.5 12a7.5 7.5 0 1 0 2.2-5.3L4.5 9M4.5 4.5V9H9M12 8v4l2.75 1.75']),
  consumers: glyph(['M4 19h16M7 19V9M12 19V5M17 19v-7']),
  meter: glyph(['M3.5 9h17v6h-17z', 'M6 12h8']),
};
