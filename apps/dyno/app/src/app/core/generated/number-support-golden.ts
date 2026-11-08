// Generated from `number_support` (dyno-core). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p dyno-core --lib widgets::number_support.

export const NUMBER_SUPPORT_GOLDEN = {
  mac: {"formats":["stacked","line"],"labelInIcon":true,"fixed":null,"note":null},
  win16: {"formats":["stacked"],"labelInIcon":false,"fixed":"Value only (label in the tooltip)","note":"Here, tray icons are 16 px, too small for a label above the number. The label shows in the tooltip; with display scaling at 150 % or more there is room for it."},
  win20: {"formats":["stacked"],"labelInIcon":false,"fixed":"Value only (label in the tooltip)","note":"Here, tray icons are 20 px, too small for a label above the number. The label shows in the tooltip; with display scaling at 150 % or more there is room for it."},
  win24: {"formats":["stacked"],"labelInIcon":true,"fixed":"Label over value","note":"Here, tray icons have one number layout: the label over the value. The label is also in the tooltip."},
} as const;
