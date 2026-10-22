/**
 * The UI ↔ backend wire contract (camelCase JSON). Mirrors the Rust DTOs in `app/src-tauri/src/dto.rs`, which mirror
 * `pitcrew_core::protocol`; change both sides in the same commit.
 */

/** Which runner implementation. */
export type RunnerKind = 'gitea-runner' | 'forgejo-runner';

/** The one-word status of a runner (`pitcrew_core::health::RunnerStatus`). */
export type RunnerStatus =
  | 'stopped'
  | 'notRegistered'
  | 'crashLoop'
  | 'updating'
  | 'busy'
  | 'stalled'
  | 'starting'
  | 'degraded'
  | 'idle';

/** How the last runner update ended (`pitcrew_core::update::UpdateOutcome`, simplified for display). */
export interface LastUpdate {
  result: 'updated' | 'rolledBack' | 'postponed' | 'cancelled' | 'failed';
  from: string | null;
  to: string;
  /** For failures, roll-backs and postponements: why, for the user. */
  reason: string | null;
  /** When it ended, in milliseconds since the Unix epoch. */
  finishedAtMs: number;
}

/** What the window shows about one runner. */
export interface RunnerSnapshot {
  name: string;
  kind: RunnerKind;
  status: RunnerStatus;
  currentVersion: string | null;
  latestVersion: string | null;
  lastUpdate: LastUpdate | null;
  /** The update in progress (`pitcrew_core::update::UpdatePhase`), `idle` when none. */
  updatePhase: string;
}

/** Whether Pitcrew can update itself (Tauri updater). */
export interface AppUpdateStatus {
  /** False in builds without the update public key. */
  available: boolean;
  /** Why not, for the user. */
  reason: string | null;
  currentVersion: string;
}

/** A command error (`app/src-tauri/src/dto.rs` `CommandError`). */
export interface CommandError {
  code: 'notImplemented' | 'serviceUnavailable' | 'protocolMismatch' | 'unknownRunner' | 'badRequest' | 'permissionDenied' | 'busy' | 'internal';
  message: string;
}
