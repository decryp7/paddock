// The wire contract of `paddock-health` (health_snapshot, health_repair, notifications_*, the `paddock://` events): the
// ts-rs output, written into this entry point (`../generated/health.ts`) so the types are used, not mirrored. Here: the
// helpers over them.
import type { NoticesSnapshot, ServiceHealthSnapshot, ServiceState } from '../generated/health';

export { SERVICE_HEALTH_EVENT, NOTIFICATIONS_EVENT, NOTIFICATIONS_OPEN_EVENT, NOTIFICATIONS_ACTION_EVENT, healthCause } from '../generated/health';
export type {
  ServiceState,
  RepairAction,
  RaiseCap,
  Cause,
  ServiceHealth,
  ServiceHealthSnapshot,
  NoticeId,
  NoticeSource,
  NoticeSeverity,
  NoticeScope,
  NoticeAction,
  NoticeLink,
  Notice,
  NoticesSnapshot,
  NoticesEvent,
  NoticeActionEvent,
  WindowNotice,
  WindowNoticeScope,
  HealthError,
  HealthErrorCode,
} from '../generated/health';
export { HEALTH_COMMANDS, type HealthCommand } from '../generated/health-commands';

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
