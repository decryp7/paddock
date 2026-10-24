// The wire contract of `paddock-health` (health_snapshot, health_repair, notifications_*, the `paddock://` events) as this
// entry point uses it. The source of truth is the ts-rs output in `shell/generated/health.ts`; ng-packagr builds each entry
// point on its own, so this entry cannot import that file and mirrors it instead. `wire.types.spec.ts` fails the type-check
// when the two drift apart (each type must be assignable both ways and the constants equal), so a Rust change that is
// not mirrored here fails `ng test`.

/** Every window hears it when a service changed; the payload is a `ServiceHealthSnapshot`. */
export const SERVICE_HEALTH_EVENT = 'paddock://service-health';

/** A window hears it when its notifications changed; the payload is a `NoticesEvent`. */
export const NOTIFICATIONS_EVENT = 'paddock://notifications';

/** Open this message (its OS notification was clicked); the payload is a `NoticeId`. */
export const NOTIFICATIONS_OPEN_EVENT = 'paddock://notifications-open';

export type ServiceState = 'ok' | 'starting' | 'degraded' | 'down' | 'quarantined' | 'repairing';

export type RepairAction = 'restart' | 'reset' | 'liftQuarantine' | 'install' | 'retry' | 'openSettings' | 'useFallback';

export interface RaiseCap {
  /** Which process, in the app's own words (`storeWorker`, `plugin:<id>`). */
  target: string;
  suggestedMb: number;
}

export interface Cause {
  /** One plain sentence. */
  message: string;
  /** The next step for this cause. */
  hint: string | null;
  /** The technical chain, for Details and bug reports. */
  detail: string | null;
  /** A stable id of the wording (`store.crashed`). */
  code: string | null;
  /** Set when a memory cap stopped the process: "Raise the cap…" is offered next to the repair. */
  raiseCap: RaiseCap | null;
  /** The Settings section that fixes it (with the `openSettings` repair). */
  settings: string | null;
}

export interface ServiceHealth {
  /** Stable id registered by the app; the notification cause is `health:<id>`. */
  service: string;
  /** What the user calls it ("the log store"). */
  label: string;
  state: ServiceState;
  /** Set while the state is not `ok`. */
  cause: Cause | null;
  /** What "Repair" does now; none when the user cannot do anything. */
  repair: RepairAction | null;
}

export interface ServiceHealthSnapshot {
  /** Grows with every change; a window keeps the newest. */
  revision: number;
  services: ServiceHealth[];
}

export type NoticeId = number;

export type NoticeSource = string;

export type NoticeSeverity = 'info' | 'success' | 'warning' | 'error' | 'critical';

export type NoticeScope = 'app' | 'project';

/** What a message's button does: a `type` the UI maps to a feature, and its arguments, flat. */
export type NoticeAction = { type: string } & { [arg: string]: unknown };

export interface NoticeLink {
  window: string | null;
  jobId: number | null;
  path: string | null;
}

export interface Notice {
  id: NoticeId;
  source: NoticeSource;
  severity: NoticeSeverity;
  title: string;
  body: string;
  code: string | null;
  /** Epoch ms of the latest occurrence. */
  at: number;
  firstAt: number;
  /** How many times it happened (a repeat of the cause merges). */
  count: number;
  read: boolean;
  primary: NoticeAction | null;
  secondary: NoticeAction | null;
  tertiary: NoticeAction | null;
  link: NoticeLink;
  /** The cause key: a second post with the same cause updates this message in place; a cause that clears removes it. */
  cause: string;
  scope: NoticeScope;
}

export interface NoticesSnapshot {
  /** Grows with every change of any list; a window keeps the newest (an answer can cross an event). */
  revision: number;
  notices: Notice[];
  unread: number;
}

export interface NoticesEvent {
  /** The message just posted or merged (the UI previews it as a toast); none for a quiet one or another change. */
  posted: Notice | null;
  revision: number;
  notices: Notice[];
  unread: number;
}

/** The notification cause of a service's outage. */
export const healthCause = (service: string): string => `health:${service}`;

/** Whether `cause` is a service outage, which the status chip states (never toasted). */
export function isHealthCause(cause: string): boolean {
  return cause.startsWith('health:');
}

/** Whether things that need the service can be asked of it now. */
export function isServing(state: ServiceState): boolean {
  return state === 'ok' || state === 'starting';
}

/** A snapshot with nothing known: nothing is claimed healthy either (see `ServiceHealthStore.isReady`). */
export const EMPTY_HEALTH: ServiceHealthSnapshot = Object.freeze({ revision: 0, services: [] }) as ServiceHealthSnapshot;

/** Nothing known yet: revision -1, so the first list or event (revision 0 and up) is taken. */
export const EMPTY_NOTICES: NoticesSnapshot = Object.freeze({ revision: -1, notices: [], unread: 0 }) as NoticesSnapshot;
