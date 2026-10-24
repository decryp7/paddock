/**
 * `@decryp7/paddock-ui/shell/notify`: notifications and service health of a window (ADR 0010, ADR 0012 §4; plan S4), the
 * port of Debrief ADR 0055. Over the `paddock-health` Tauri adapter (`health_snapshot`, `health_repair`,
 * `notifications_*`, events on `paddock://service-health` and `paddock://notifications`): the backends (Tauri and in
 * memory), `ServiceHealthStore` (nothing healthy until the first snapshot), `NotificationsStore` (the centre plus the UI's
 * own notices from `SHELL_PORTS.notify`), the one `pd-notification-host` (one toast per cause with its count, details
 * folded, host-owned time, never a `health:` cause), the centre panel and its status item, the service chip with Repair
 * (rule 19), `pd-waiting-for` and `pd-service-error`.
 */
export {
  SERVICE_HEALTH_EVENT,
  NOTIFICATIONS_EVENT,
  NOTIFICATIONS_OPEN_EVENT,
  EMPTY_HEALTH,
  EMPTY_NOTICES,
  healthCause,
  isHealthCause,
  isServing,
  type ServiceState,
  type RepairAction,
  type RaiseCap,
  type Cause,
  type ServiceHealth,
  type ServiceHealthSnapshot,
  type NoticeId,
  type NoticeSource,
  type NoticeSeverity,
  type NoticeScope,
  type NoticeAction,
  type NoticeLink,
  type Notice,
  type NoticesSnapshot,
  type NoticesEvent,
  type WindowNotice,
  type WindowNoticeScope,
  type HealthError,
  type HealthErrorCode,
  HEALTH_COMMANDS,
  type HealthCommand,
} from './src/wire.types';
export { parseHealthSnapshot, parseNotice, parseNoticesSnapshot, parseNoticesEvent } from './src/wire';
export {
  ServiceHealthBackend,
  NotificationsBackend,
  TauriServiceHealthBackend,
  TauriNotificationsBackend,
  MemoryServiceHealth,
  MemoryNotificationCentre,
  type NotifyIpc,
  type NotifyCommand,
  type Unlisten,
  type MemoryPost,
  type MemoryServiceSpec,
  type MemoryRepairOutcome,
} from './src/backends';
export {
  NOTIFY_CLOCK,
  OPEN_SETTINGS,
  RAISE_CAP,
  NOTICE_ACTION_HANDLERS,
  TOAST_MS,
  MAX_VISIBLE,
  type NotifyClock,
  type NoticeActionHandler,
} from './src/notify-config';
export { ServiceHealthStore, type ServiceHealthStoreType } from './src/service-health.store';
export { NotificationsStore, localCause, LOCAL_CAUSE, LOCAL_SOURCE, type NotificationsStoreType, type ReadFilter } from './src/notifications.store';
export { NoticeActions, repairLabel, repairButtonLabel } from './src/notice-actions';
export {
  NOTIFICATION_SOURCES,
  visibleNotifications,
  persists,
  type HostAction,
  type HostNotification,
  type NotificationSource,
} from './src/notification-host.model';
export { NotificationsSource } from './src/notifications-source';
export { PdNotificationHost } from './src/notification-host';
// The status items load on first show (chunks of their own): their classes are exported as types only (plan S16).
export type { PdServiceStatus } from './src/service-status';
export { chipText, chipCompactText, shortName, stateWord } from './src/service-status-text';
export { PdWaitingFor, PdServiceError, offlineLine, serviceStatus, waitingKind, type WaitingKind } from './src/waiting-for';
export type { PdNotificationsPanel } from './src/notifications-panel';
export type { PdNotificationsStatus } from './src/notifications-status';
export { titleWithCount, noticeTime, notificationsStatusText } from './src/notice-text';
export {
  provideShellNotify,
  provideNotificationSource,
  provideNoticeAction,
  notifyPort,
  type ShellNotifyOptions,
} from './src/provide-notify';
export { NOTIFY_CONTRIBUTIONS, OS_NOTIFICATIONS_CONTRIBUTIONS, NOTIFICATIONS_SETTINGS_PAGE } from './src/notify.contributions';
