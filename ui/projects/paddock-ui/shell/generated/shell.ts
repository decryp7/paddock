// Generated from the Rust wire types of paddock-shell (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-shell --lib generated.

/** The plugin's name: its commands are `plugin:paddock-shell|<command>`. */
export const SHELL_PLUGIN = 'paddock-shell';

/** The shell's version: the plugin's `shell_version` must answer it (one version across the boundary). */
export const SHELL_VERSION = '0.3.0';

export type AppIdentity = { id: string, name: string, version: string, };

export type VersionReply = { 
/**
 * The `paddock-shell` crate's version (lockstep with `@decryp7/paddock-ui`).
 */
shell: string, app: AppIdentity, };
