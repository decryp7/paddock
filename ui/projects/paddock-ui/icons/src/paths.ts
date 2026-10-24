/**
 * The generic glyphs every app used to redraw (plan X36), on the 24 px grid with a 3 px safe margin: each value is the
 * path data of one glyph (sub-paths joined in one string), drawn by `outlineIcon`. Names are what an app writes after
 * the namespace: `pd:refresh`. Dots are drawn as small circles, never zero-length lines, so they print at the same size
 * everywhere.
 */
export const PD_ICON_PATHS = {
  // Actions
  refresh: 'M20 12a8 8 0 1 1-2.34-5.66L20 8.5M20 4v4.5h-4.5',
  play: 'M7.5 4.75v14.5L19 12z',
  pause: 'M8.5 5v14M15.5 5v14',
  stop: 'M6.5 6.5h11v11h-11z',
  trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 12.5h10L18 7M9 7V4.5h6V7',
  search: 'M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM15.25 15.25 20 20',
  copy: 'M9 9h11v11H9zM15 9V4H4v11h5',
  plus: 'M12 5v14M5 12h14',
  minus: 'M5 12h14',
  close: 'M6 6l12 12M18 6 6 18',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  more: 'M5 10.75a1.25 1.25 0 1 0 0 2.5 1.25 1.25 0 0 0 0-2.5ZM12 10.75a1.25 1.25 0 1 0 0 2.5 1.25 1.25 0 0 0 0-2.5ZM19 10.75a1.25 1.25 0 1 0 0 2.5 1.25 1.25 0 0 0 0-2.5Z',
  download: 'M12 4v11M7.5 10.5 12 15l4.5-4.5M4.5 19.5h15',
  edit: 'M4.5 19.5l.9-4.2L16.6 4.1a2.1 2.1 0 0 1 3 3L8.4 18.3zM14.5 6.2l3.3 3.3',
  external: 'M13.5 4.5h6v6M19.5 4.5 11 13M17.5 14v5a1 1 0 0 1-1 1h-10a1 1 0 0 1-1-1v-10a1 1 0 0 1 1-1h5',
  pin: 'M9 4h6l-1 5.5 3 3H7l3-3zM12 12.5V20',
  // Status
  alert: 'M12 4 21 19.5H3zM12 10v4.5M12 17.25a.25.25 0 1 0 0 .5.25.25 0 0 0 0-.5Z',
  info: 'M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17ZM12 11v5.5M12 7.25a.25.25 0 1 0 0 .5.25.25 0 0 0 0-.5Z',
  lock: 'M5.5 11h13v9.5h-13zM8 11V7.5a4 4 0 0 1 8 0V11M12 15v2',
  shield: 'M12 3 19.5 6v5.5c0 4.6-3.1 8-7.5 9.5-4.4-1.5-7.5-4.9-7.5-9.5V6z',
  clock: 'M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17ZM12 7.5V12l3 2',
  bell: 'M6 10.5a6 6 0 0 1 12 0V16l1.5 2h-15L6 16zM10 20.5a2 2 0 0 0 4 0',
  // Things
  folder: 'M3.5 6.5a1 1 0 0 1 1-1H9l2 2.5h8.5a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-15a1 1 0 0 1-1-1z',
  globe:
    'M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17ZM3.5 12h17M12 3.5c2.5 2.4 3.7 5.2 3.7 8.5S14.5 18.1 12 20.5M12 3.5C9.5 5.9 8.3 8.7 8.3 12s1.2 6.1 3.7 8.5',
  sun: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM12 2.5v2.5M12 19v2.5M2.5 12H5M19 12h2.5M5.3 5.3l1.8 1.8M16.9 16.9l1.8 1.8M5.3 18.7l1.8-1.8M16.9 7.1l1.8-1.8',
  moon: 'M20 14.5A8.5 8.5 0 0 1 9.5 4 8.5 8.5 0 1 0 20 14.5Z',
  settings: 'M10.07 5.69L10.56 2.91L13.44 2.91L13.93 5.69L15.1 6.17L17.41 4.56L19.44 6.59L17.83 8.9L18.31 10.07L21.09 10.56L21.09 13.44L18.31 13.93L17.83 15.1L19.44 17.41L17.41 19.44L15.1 17.83L13.93 18.31L13.44 21.09L10.56 21.09L10.07 18.31L8.9 17.83L6.59 19.44L4.56 17.41L6.17 15.1L5.69 13.93L2.91 13.44L2.91 10.56L5.69 10.07L6.17 8.9L4.56 6.59L6.59 4.56L8.9 6.17ZM12 9.25a2.75 2.75 0 1 0 0 5.5 2.75 2.75 0 0 0 0-5.5Z',
  // Navigation and window
  'chevron-up': 'M6 15l6-6 6 6',
  'chevron-down': 'M6 9l6 6 6-6',
  'chevron-left': 'M15 6l-6 6 6 6',
  'chevron-right': 'M9 6l6 6-6 6',
  home: 'M4 11 12 4l8 7M6 9.5V20h4v-5h4v5h4V9.5',
  menu: 'M4 6h16M4 12h16M4 18h16',
  sidebar: 'M4 5h16v14H4zM9.5 5v14',
  window: 'M3.5 5h17v14h-17zM3.5 9h17',
  keyboard:
    'M2.5 6.5h19v11h-19zM6 9.75a.25.25 0 1 0 0 .5.25.25 0 0 0 0-.5ZM10 9.75a.25.25 0 1 0 0 .5.25.25 0 0 0 0-.5ZM14 9.75a.25.25 0 1 0 0 .5.25.25 0 0 0 0-.5ZM18 9.75a.25.25 0 1 0 0 .5.25.25 0 0 0 0-.5ZM7.5 14h9',
  activity: 'M2.5 12h4l3-7.5 4 15 3-7.5h5',
} as const satisfies Readonly<Record<string, string>>;

/** The name of a glyph in the set (the part after `pd:`). */
export type PdIconName = keyof typeof PD_ICON_PATHS;
