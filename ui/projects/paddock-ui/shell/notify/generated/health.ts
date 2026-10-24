// Generated from the Rust wire types of paddock-health (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-health --all-features --lib generated.

/** Every window hears it when a service changed; the payload is a `ServiceHealthSnapshot`. */
export const SERVICE_HEALTH_EVENT = 'paddock://service-health';

/** A window hears it when its notifications changed; the payload is a `NoticesEvent`. */
export const NOTIFICATIONS_EVENT = 'paddock://notifications';

/** Open this message (its OS notification was clicked); the payload is a `NoticeId`. */
export const NOTIFICATIONS_OPEN_EVENT = 'paddock://notifications-open';

/** Run this action of a message (a button of its OS notification that only a window can run); the payload is a `NoticeActionEvent`. */
export const NOTIFICATIONS_ACTION_EVENT = 'paddock://notifications-action';

/** The notification cause of a service's outage. */
export const healthCause = (service: string): string => `health:${service}`;

/**
 * What a message's button does: a `type` the UI maps to a feature, and its arguments, flat
 * (`{ type: 'repairService', service: 'store' }`).
 */
export type NoticeAction = { type: string } & { [arg: string]: unknown };

export type ServiceState = "ok" | "starting" | "degraded" | "down" | "quarantined" | "repairing";

export type RepairAction = "restart" | "reset" | "liftQuarantine" | "install" | "retry" | "openSettings" | "useFallback" | "openLoginItems";

export type RaiseCap = { 
/**
 * Which process, in the app's own words (`storeWorker`, `plugin:<id>`).
 */
target: string, suggestedMb: number, };

export type Cause = { 
/**
 * One plain sentence.
 */
message: string, 
/**
 * The next step for this cause.
 */
hint: string | null, 
/**
 * The technical chain, for Details and bug reports.
 */
detail: string | null, 
/**
 * A stable id of the wording (`store.crashed`), so the text can be translated later without a wire change.
 */
code: string | null, 
/**
 * Set when a memory cap stopped the process: the UI offers "Raise the cap…" next to the repair.
 */
raiseCap: RaiseCap | null, 
/**
 * The Settings section that fixes it (with the `openSettings` repair).
 */
settings: string | null, };

export type ServiceHealth = { 
/**
 * Stable id registered by the app (`store`, `llm`, ...); the notification cause is `health:<id>`.
 */
service: string, 
/**
 * What the user calls it ("the log store").
 */
label: string, state: ServiceState, 
/**
 * Set while the state is not `ok`.
 */
cause: Cause | null, 
/**
 * What "Repair" does now; none when the user cannot do anything.
 */
repair: RepairAction | null, };

export type ServiceHealthSnapshot = { revision: number, services: Array<ServiceHealth>, };

export type NoticeId = number;

export type NoticeSource = string;

export type NoticeSeverity = "info" | "success" | "warning" | "error" | "critical";

export type NoticeScope = "app" | "project";

export type NoticeLink = { 
/**
 * The window it was posted for.
 */
window: string | null, 
/**
 * The job it is about (`paddock-jobs`).
 */
jobId: number | null, 
/**
 * The file or folder it is about.
 */
path: string | null, };

export type Notice = { id: NoticeId, source: NoticeSource, severity: NoticeSeverity, title: string, body: string, 
/**
 * A stable id of the wording, for translation later.
 */
code: string | null, 
/**
 * Epoch ms of the latest occurrence.
 */
at: number, firstAt: number, 
/**
 * How many times it happened (a repeat of the cause merges).
 */
count: number, read: boolean, primary: NoticeAction | null, secondary: NoticeAction | null, 
/**
 * A third button (e.g. "Skip this version" next to "Update" and "Later").
 */
tertiary: NoticeAction | null, link: NoticeLink, 
/**
 * The cause key (`health:store`, `job:12`, `update:1.2`): a second post with the same cause updates this message in
 * place; a cause that clears resolves (removes) it.
 */
cause: string, scope: NoticeScope, };

export type NoticesSnapshot = { 
/**
 * Grows with every change of any list; a window keeps the newest (an answer to `notifications_list` can cross an
 * event). It is read before the lists, so a snapshot holds at least the changes up to it.
 */
revision: number, notices: Array<Notice>, unread: number, };

export type NoticesEvent = { 
/**
 * The message just posted or merged (the UI previews it as a toast); none for a quiet one or another change.
 */
posted: Notice | null, 
/**
 * Grows with every change of any list; a window keeps the newest (an answer to `notifications_list` can cross an
 * event). It is read before the lists, so a snapshot holds at least the changes up to it.
 */
revision: number, notices: Array<Notice>, unread: number, };

export type NoticeActionEvent = { 
/**
 * The message the button belongs to.
 */
id: NoticeId, 
/**
 * The action, as the message offered it.
 */
action: NoticeAction, };

export type RestoredNotices = { snapshot: NoticesSnapshot, warning: string | null, };

export type NoticesRecord = { version: number, notices: Array<Notice>, };

export type WindowNoticeScope = "app" | "window";

export type WindowNotice = { 
/**
 * `info`, `success`, `warning` or `error` (`critical` is the engine's).
 */
severity: NoticeSeverity, 
/**
 * One sentence for the user (1 to [`MAX_TITLE`] characters).
 */
title: string, 
/**
 * More detail (at most [`MAX_BODY`] characters).
 */
body?: string, 
/**
 * What kind of notice (`[a-z0-9.-]`, at most [`MAX_KIND`]); its source in the list. Not `health`.
 */
kind?: string, scope?: WindowNoticeScope, };

export type HealthErrorCode = "unavailable" | "invalid" | "refused";

export type HealthError = { code: HealthErrorCode, 
/**
 * For the user, as is.
 */
message: string, 
/**
 * Whether the same call may work later (only `unavailable`: the shell is starting).
 */
retryable: boolean, };
