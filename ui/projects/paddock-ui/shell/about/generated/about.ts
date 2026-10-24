// Generated from the Rust wire types of paddock-updater (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-updater --lib generated.

export type UpdateErrorKind = "notAvailable" | "offline" | "unreachable" | "noRelease" | "noPlatform" | "invalidManifest" | "signatureInvalid" | "tooLarge" | "diskFull" | "storage" | "installFailed" | "busy" | "invalid";

export type UpdateErrorDto = { kind: UpdateErrorKind, message: string, nextStep: string, detail: string | null, };

export type UpdateInfo = { version: string, currentVersion: string, 
/**
 * Plain text (never markup), bounded.
 */
notes: string, 
/**
 * RFC 3339.
 */
pubDate: string | null, 
/**
 * Download size in bytes, when the release says.
 */
size: number | null, };

export type UpdatePhase = { "kind": "unavailable" } | { "kind": "idle" } | { "kind": "checking", job: number | null, } | { "kind": "upToDate" } | { "kind": "available", info: UpdateInfo, } | { "kind": "downloading", info: UpdateInfo, job: number | null, } | { "kind": "ready", info: UpdateInfo, 
/**
 * Install when the app quits instead of now.
 */
installOnQuit: boolean, } | { "kind": "installing", info: UpdateInfo, } | { "kind": "failed", error: UpdateErrorDto, info: UpdateInfo | null, };

export type ChannelInfo = { id: string, label: string, };

export type UpdateStatus = { 
/**
 * The running version (rule 6: the UI shows this, never a copy of its own).
 */
currentVersion: string, phase: UpdatePhase, 
/**
 * Seconds since 1970 of the last finished check.
 */
lastChecked: number | null, autoCheck: boolean, channel: string, channels: Array<ChannelInfo>, 
/**
 * The host a check contacts (`None` in a build without updates).
 */
endpointHost: string | null, releasesPage: string | null, };

export type Preferences = { 
/**
 * Check a minute after start and then daily (off by default; nothing is downloaded without asking).
 */
autoCheck: boolean, 
/**
 * A channel id of [`UpdateStatus::channels`].
 */
channel: string, };

export type LegalDocInfo = { id: string, title: string, };

export type AboutInfo = { appName: string, 
/**
 * The running version (the app's package version).
 */
version: string, 
/**
 * `macos`, `windows`, `linux`.
 */
os: string, arch: string, 
/**
 * The commit the build was made from, when the build says.
 */
commit: string | null, 
/**
 * The build date (`YYYY-MM-DD`), when the build says.
 */
built: string | null, homepage: string | null, releasesPage: string | null, 
/**
 * The legal documents [`About::legal`] serves, in order.
 */
legal: Array<LegalDocInfo>, 
/**
 * Whether this build can update itself (it has an update key and a release address).
 */
updatesAvailable: boolean, };

export type NotesGroup = { title: string, items: Array<string>, };

export type ReleaseNotes = { version: string, 
/**
 * `YYYY-MM-DD`, when the heading has one.
 */
date: string | null, groups: Array<NotesGroup>, };

export type WhatsNew = { version: string, release: ReleaseNotes | null, };

export type WhatsNewDecision = "show" | "remember" | "nothing";

/** The event every `UpdateStatus` is sent on, to every window. */
export const UPDATES_EVENT = 'paddock://updates';

/** The job phase of the signature check (bytes). */
export const SIGNATURE_PHASE = 'signature';
