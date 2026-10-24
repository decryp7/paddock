// Generated from the command table by paddock-commands. Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-mcp --lib generated.

/** Every command of the table: its kind, the window classes that may call it and its largest request in bytes. */
export const MCP_COMMANDS = {
  mcp_audit: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  mcp_audit_clear: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  mcp_get: { kind: 'query', windows: ['main', 'popout', 'dialog'], limit: 262144 },
  mcp_issue_key: { kind: 'mutation', windows: ['dialog'], limit: 262144 },
  mcp_revoke_key: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
  mcp_save: { kind: 'mutation', windows: ['main', 'dialog'], limit: 262144 },
} as const;

/** A command name of the table. */
export type McpCommand = keyof typeof MCP_COMMANDS;
