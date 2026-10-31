// Generated from Spotter's widget wire types (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p spotter-core --lib widgets::generated.

export type SpotterSurface = "popover";

export type PopoverWidget = "watchedSites" | "recentChanges" | "nextCheck" | "quietHours" | "failures" | "notifications";

export type LoadingRows = { 
/**
 * The widget kind.
 */
kind: PopoverWidget, 
/**
 * How many placeholder rows its loading twin draws.
 */
rows: number, };

export type SiteState = "ok" | "new" | "checking" | "paused" | "attention" | "pending";

export type SiteRow = { 
/**
 * The watch id.
 */
id: string, 
/**
 * The watch name.
 */
name: string, 
/**
 * How it is doing.
 */
state: SiteState, 
/**
 * The newest version, when there is one.
 */
version: string | null, };

export type ChangeRow = { 
/**
 * The watch id.
 */
id: string, 
/**
 * The watch name.
 */
name: string, 
/**
 * The version found.
 */
version: string, 
/**
 * When Spotter first saw it, in milliseconds since the epoch.
 */
firstSeenMs: number, 
/**
 * Nobody has seen it yet.
 */
unseen: boolean, };

export type FailureKind = "unreachable" | "authNeeded" | "needsYou" | "invalid" | "failed";

export type FailureRow = { 
/**
 * The watch id.
 */
id: string, 
/**
 * The watch name.
 */
name: string, 
/**
 * Why it failed.
 */
kind: FailureKind, 
/**
 * The engine's message for the person.
 */
message: string, 
/**
 * Failed checks in a row.
 */
failures: number, };

export type NotificationMode = { "kind": "on" } | { "kind": "paused", 
/**
 * When they resume, in milliseconds since the epoch.
 */
untilMs: number, } | { "kind": "off" };

export type PopoverValue = { "kind": "watchedSites", 
/**
 * All watches.
 */
total: number, 
/**
 * The first few, attention first.
 */
rows: Array<SiteRow>, 
/**
 * Watches not listed.
 */
more: number, } | { "kind": "recentChanges", 
/**
 * Newest first.
 */
rows: Array<ChangeRow>, 
/**
 * Releases not listed.
 */
more: number, 
/**
 * Watches with a release nobody has seen yet (the widget's head says "3 new").
 */
unseen: number, } | { "kind": "nextCheck", 
/**
 * The watch id.
 */
id: string, 
/**
 * The watch name.
 */
name: string, 
/**
 * When, in milliseconds since the epoch.
 */
atMs: number, 
/**
 * Other watches due at the same time or earlier than a minute after it.
 */
alsoDue: number, } | { "kind": "quietHours", 
/**
 * Start, `HH:MM` (local).
 */
start: string, 
/**
 * End, `HH:MM` (local).
 */
end: string, 
/**
 * Whether they are in force right now.
 */
active: boolean, } | { "kind": "failures", 
/**
 * Most failed checks first.
 */
rows: Array<FailureRow>, 
/**
 * Failing watches not listed.
 */
more: number, } | { "kind": "notifications", 
/**
 * On, paused or off.
 */
mode: NotificationMode, };

export type WidgetActionId = { "type": "openSpotter" } | { "type": "openWatch", 
/**
 * The watch id.
 */
id: string, } | { "type": "openWatches" } | { "type": "addWatch" } | { "type": "retryCheck", 
/**
 * The watch id.
 */
id: string, } | { "type": "checkAll" } | { "type": "resumeNotifications" } | { "type": "openSettings" };
