// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-service --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const SERVICE_COMMANDS = {
  open_login_items: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  service_install: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  service_repair: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  service_status: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type ServiceCommand = keyof typeof SERVICE_COMMANDS;
