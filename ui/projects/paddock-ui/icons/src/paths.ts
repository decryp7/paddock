/**
 * The generic glyphs every app used to redraw (plan X36), on the 24 px grid with a 2.5 px safe margin (3 px for all but the wide
 * glyphs `keyboard`, `activity` and `sun`): each value is the path data of one glyph (sub-paths joined in one string),
 * drawn by `outlineIcon`. Names are what an app writes after the namespace: `pd:refresh`. Dots are never stroked (a
 * stroke wider than the circle's diameter leaves a hollow ring): they are the filled circles of `PD_ICON_DOTS`.
 */
export const PD_ICON_PATHS = {
  // Actions
  refresh: 'M20 12a8 8 0 1 1-2.34-5.66L20 8.5M20 4v4.5h-4.5',
  play: 'M7.5 4.75v14.5L19 12z',
  pause: 'M8.5 5v14M15.5 5v14',
  stop: 'M6.25 6.25h11.5v11.5H6.25z',
  trash: 'M4 7h16M10 11v6M14 11v6M6 7l1 12.5h10L18 7M9 7V4.5h6V7',
  search: 'M10.5 4a6.5 6.5 0 1 0 0 13 6.5 6.5 0 0 0 0-13ZM15.25 15.25 20 20',
  copy: 'M9 9h11v11H9zM15 9V4H4v11h5',
  plus: 'M12 5v14M5 12h14',
  minus: 'M5 12h14',
  close: 'M6 6l12 12M18 6 6 18',
  check: 'M5 12.5l4.5 4.5L19 7.5',
  more: '',
  download: 'M12 4v11M7.5 10.5 12 15l4.5-4.5M4.5 19.5h15',
  edit: 'M4.5 19.5l.9-4.2L16.6 4.1a2.1 2.1 0 0 1 3 3L8.4 18.3zM14.5 6.2l3.3 3.3',
  external: 'M13.5 4.5h6v6M19.5 4.5 11 13M17.5 14v5a1 1 0 0 1-1 1h-10a1 1 0 0 1-1-1v-10a1 1 0 0 1 1-1h5',
  pin: 'M9 4h6l-1 5.5 3 3H7l3-3zM12 12.5V20',
  // Status
  alert: 'M12 4 21 19.5H3zM12 9v3.5',
  // A status error (circle with ×); `alert` (triangle) is the warning shape, `info` (circle i) the info one.
  error: 'M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17ZM9 9l6 6M15 9l-6 6',
  info: 'M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17ZM12 11v5.5',
  lock: 'M5.5 11h13v9.5h-13zM8 11V7.5a4 4 0 0 1 8 0V11M12 15v2',
  shield: 'M12 3 19.5 6v5.5c0 4.6-3.1 8-7.5 9.5-4.4-1.5-7.5-4.9-7.5-9.5V6z',
  clock: 'M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17ZM12 7.5V12l3 2',
  bell: 'M6 10.5a6 6 0 0 1 12 0V16l1.5 2h-15L6 16zM10 20.5a2 2 0 0 0 4 0',
  // Things
  // File kinds (plan X31 follow-up): beside `file` (below, from the workbench) a page with lines, a page with code brackets, an archive box.
  'file-text': 'M5.75 3.5h7.75l4.75 5v12H5.75zM13.5 3.5v5h4.75M8.75 12.5h6.5M8.75 16h6.5',
  'file-code': 'M5.75 3.5h7.75l4.75 5v12H5.75zM13.5 3.5v5h4.75M9.75 11.5 7.25 14l2.5 2.5M13.75 11.5l2.5 2.5-2.5 2.5',
  archive: 'M4 5h16v3.5H4zM5.5 8.5v11h13v-11M10 12.5h4',
  // Reveal in the file manager: a folder with an arrow out of it (the kind `folder` stays the kind).
  reveal: 'M12.5 18.5h-8a1 1 0 0 1-1-1v-11a1 1 0 0 1 1-1H9l2 2.5h8.5a1 1 0 0 1 1 1V12M15 17.5h5M17.5 15l2.5 2.5-2.5 2.5',
  folder: 'M3.5 6.5a1 1 0 0 1 1-1H9l2 2.5h8.5a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-15a1 1 0 0 1-1-1z',
  globe:
    'M12 3.5a8.5 8.5 0 1 0 0 17 8.5 8.5 0 0 0 0-17ZM3.5 12h17M12 3.5c-2.3 2.4-3.5 5.2-3.5 8.5s1.2 6.1 3.5 8.5c2.3-2.4 3.5-5.2 3.5-8.5S14.3 5.9 12 3.5Z',
  sun: 'M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8ZM12 2.5v2.5M12 19v2.5M2.5 12H5M19 12h2.5M5.3 5.3l1.8 1.8M16.9 16.9l1.8 1.8M5.3 18.7l1.8-1.8M16.9 7.1l1.8-1.8',
  moon: 'M20 14.5A8.5 8.5 0 0 1 9.5 4 8.5 8.5 0 1 0 20 14.5Z',
  settings:
    'M10.1 5.68L10.5 3.13L13.5 3.13L13.9 5.68L16.52 7.19L18.94 6.26L20.44 8.86L18.42 10.49L18.42 13.51L20.44 15.14L18.94 17.74L16.52 16.81L13.9 18.32L13.5 20.87L10.5 20.87L10.1 18.32L7.48 16.81L5.06 17.74L3.56 15.14L5.58 13.51L5.58 10.49L3.56 8.86L5.06 6.26L7.48 7.19ZM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6Z',
  // Navigation and window
  'chevron-up': 'M6 15l6-6 6 6',
  'chevron-down': 'M6 9l6 6 6-6',
  'chevron-left': 'M15 6l-6 6 6 6',
  'chevron-right': 'M9 6l6 6-6 6',
  home: 'M4 11 12 4l8 7M6 9.5V20h4v-5h4v5h4V9.5',
  menu: 'M4 6h16M4 12h16M4 18h16',
  sidebar: 'M4 5h16v14H4zM9.5 5v14',
  window: 'M3.5 5h17v14h-17zM3.5 9h17',
  keyboard: 'M2.5 6.5h19v11h-19zM7.5 14h9',
  columns: 'M4 5h16v14H4zM9.3 5v14M14.7 5v14',
  sort: 'M8 9l4-4 4 4M8 15l4 4 4-4',
  activity: 'M2.5 12h4l3-7.5 4 15 3-7.5h5',
  // Workbench (plan X50, review 1): the assistant, its history, a window kept on top, a panel docked back.
  chat: 'M4.5 5.5h15v10h-8l-4.5 3.5v-3.5h-2.5z',
  history: 'M4.5 12a7.5 7.5 0 1 0 2.2-5.3L4.5 9M4.5 4.5V9H9M12 8v4l2.75 1.75',
  layers: 'M12 4 20 8.5 12 13 4 8.5zM5.5 12.5 12 16.2l6.5-3.7M5.5 16 12 19.7l6.5-3.7',
  'dock-in': 'M3.5 5h17v14h-17zM12 5v6.5M9 8.5l3 3 3-3M7 15h10',
  file: 'M5.75 3.5h7.75l4.75 5v12H5.75zM13.5 3.5v5h4.75',
  // Code symbols (plan X50, review 2 P3-5): the kind of a symbol in a palette row or an outline, instead of a letter badge.
  'symbol-function': 'M16 4.5h-1.5C12.8 4.5 12 5.4 12 7.2v10.1c0 1.6-.8 2.2-2.3 2.2H8M8.5 10.5h7',
  'symbol-type': 'M9 4.5H8c-1.4 0-2 .7-2 2V10l-1.5 2L6 14v3.5c0 1.3.6 2 2 2h1M15 4.5h1c1.4 0 2 .7 2 2V10l1.5 2-1.5 2v3.5c0 1.3-.6 2-2 2h-1',
  'symbol-module': 'M12 3.5 19.5 7.75v8.5L12 20.5 4.5 16.25v-8.5zM4.5 7.75 12 12l7.5-4.25M12 12v8.5',
} as const satisfies Readonly<Record<string, string>>;

/** A filled dot of a glyph: centre and radius on the 24 px grid. */
export type PdIconDot = readonly [cx: number, cy: number, r: number];

/**
 * The dots of the glyphs that have some (the "!" and "i" dots, the keys, "more"), drawn as filled circles with no stroke,
 * so every engine prints them solid at every size. A glyph with dots keeps its lines in `PD_ICON_PATHS`.
 */
export const PD_ICON_DOTS: Readonly<Partial<Record<PdIconName, readonly PdIconDot[]>>> = {
  more: [
    [5, 12, 1.5],
    [12, 12, 1.5],
    [19, 12, 1.5],
  ],
  alert: [[12, 15.75, 0.9]],
  info: [[12, 7.5, 1]],
  keyboard: [
    [6, 10, 0.9],
    [10, 10, 0.9],
    [14, 10, 0.9],
    [18, 10, 0.9],
  ],
};

/** The name of a glyph in the set (the part after `pd:`). */
export type PdIconName = keyof typeof PD_ICON_PATHS;
