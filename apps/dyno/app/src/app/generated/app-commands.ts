// Generated from the command table by paddock-commands. Do not edit: cd app/src-tauri && PADDOCK_WRITE_TS=1 cargo test --test shell.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const APP_COMMANDS = {
  dyno: { kind: 'mutation', windows: ['main', 'dialog', 'tray'], limit: 262144 },
  edit_widget_layout: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  metric_readings: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
  open_taskbar_settings: { kind: 'mutation', windows: ['main'], limit: 262144 },
  pick_folder: { kind: 'mutation', windows: ['main'], limit: 262144 },
  popover_preview: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
  popover_snapshot: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
  popover_values: { kind: 'query', windows: ['main', 'dialog', 'tray'], limit: 262144 },
  preview_widget_layout: { kind: 'query', windows: ['main'], limit: 262144 },
  quit_and_stop_measuring: { kind: 'mutation', windows: ['main', 'tray'], limit: 262144 },
  quit_keeps_measuring: { kind: 'query', windows: ['main', 'tray'], limit: 262144 },
  run_widget_action: { kind: 'mutation', windows: ['main', 'tray'], limit: 262144 },
  save_benchmarks: { kind: 'mutation', windows: ['main'], limit: 262144 },
  save_diagnostics: { kind: 'mutation', windows: ['main'], limit: 262144 },
  save_inventory_report: { kind: 'mutation', windows: ['main'], limit: 262144 },
  save_issues: { kind: 'mutation', windows: ['main'], limit: 262144 },
  save_report: { kind: 'mutation', windows: ['main'], limit: 262144 },
  take_navigation: { kind: 'mutation', windows: ['main'], limit: 262144 },
  tray_availability: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
  widget_choices: { kind: 'query', windows: ['main', 'dialog'], limit: 262144 },
  widget_layout: { kind: 'query', windows: ['main', 'dialog', 'tray'], limit: 262144 },
} as const;

/** A command name of the table. */
export type AppCommand = keyof typeof APP_COMMANDS;
