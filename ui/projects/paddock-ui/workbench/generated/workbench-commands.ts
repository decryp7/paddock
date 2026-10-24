// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-workbench --all-features --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const WORKBENCH_COMMANDS = {
  assistant_approve_tool: { kind: 'mutation', windows: ['main', 'popout'], limit: 262144 },
  assistant_conversations: { kind: 'query', windows: ['main', 'popout'], limit: 262144 },
  assistant_delete: { kind: 'mutation', windows: ['main', 'popout'], limit: 262144 },
  assistant_info: { kind: 'query', windows: ['main', 'popout'], limit: 262144 },
  assistant_load: { kind: 'query', windows: ['main', 'popout'], limit: 262144 },
  assistant_send: { kind: 'mutation', windows: ['main', 'popout'], limit: 1048576 },
  assistant_stop: { kind: 'mutation', windows: ['main', 'popout'], limit: 262144 },
  drag_ghost_end: { kind: 'mutation', windows: ['main'], limit: 262144 },
  drag_ghost_show: { kind: 'quiet', windows: ['main'], limit: 262144 },
  drag_ghost_start: { kind: 'mutation', windows: ['main'], limit: 3145728 },
  layout_get: { kind: 'query', windows: ['main', 'popout'], limit: 262144 },
  layout_set: { kind: 'quiet', windows: ['main'], limit: 1114112 },
  panel_dock_back: { kind: 'mutation', windows: ['main', 'popout'], limit: 262144 },
  panel_focus: { kind: 'mutation', windows: ['main', 'popout'], limit: 262144 },
  panel_keep_on_top: { kind: 'mutation', windows: ['popout'], limit: 262144 },
  panel_pop_out: { kind: 'mutation', windows: ['main'], limit: 16384 },
  panels_list: { kind: 'query', windows: ['main', 'popout'], limit: 262144 },
  pointer_button_down: { kind: 'query', windows: ['main', 'popout'], limit: 262144 },
  ui_sync_publish: { kind: 'quiet', windows: ['main', 'popout', 'dialog'], limit: 655360 },
} as const;

/** A command name of the table. */
export type WorkbenchCommand = keyof typeof WORKBENCH_COMMANDS;
