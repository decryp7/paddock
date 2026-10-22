/**
 * The UI ↔ backend wire contract (camelCase JSON). Mirrored by the Rust DTOs in `app/src-tauri/src/dto.rs`;
 * change both sides in the same commit. Times are milliseconds since the Unix epoch (UTC).
 */

/** Colour theme. */
export type Theme = 'system' | 'light' | 'dark';

/** A watch's status after its last check (`spotter_core::state::WatchStatus`). */
export type WatchStatusKind = 'pending' | 'ok' | 'noMatch' | 'unreachable' | 'authNeeded' | 'needsYou' | 'invalid' | 'failed';

export interface WatchStatus {
  kind: WatchStatusKind;
  /** Why, for failures (written for the user by the engine). `needsYou`: a page asked for a login or captcha. */
  message: string | null;
}

/** One release. */
export interface Release {
  /** The version as the watch's scheme shows it (`1.4.2-rc.1`, `R1.01.00T10`). */
  version: string;
  prerelease: boolean;
  name: string;
  /** A URL or a path. */
  location: string;
  publishedAtMs: number | null;
  /** When Spotter first found it. */
  firstSeenMs: number;
  size: number | null;
  sha256: string | null;
}

/** Version schemes (`spotter_core::version::VersionScheme`). */
export type VersionSchemeId = 'semver' | 'numeric' | 'build-number' | 'date' | 'custom';

export type NotifyMode = 'every-new' | 'newer-than-installed' | 'off';

export interface QuietHours {
  /** `HH:MM`, local time. */
  start: string;
  end: string;
}

export interface NotificationPolicy {
  mode: NotifyMode;
  /** Repeat at every check until acknowledged. */
  nag: boolean;
  quietHours: QuietHours | null;
}

/** An action on a new release. `run-command` never goes through a shell. */
export type ChannelKind = 'webhook' | 'ntfy' | 'email';

/** An extra place to send a notification to (off unless added). The address/URL is stored in the config; secrets are not. */
export interface NotificationChannel {
  kind: ChannelKind;
  target: string;
}

/** Narrows which versions count (`spotter_core::release::ReleaseFilter`). */
export interface VersionFilter {
  /** Only versions matching this prefix (`1.` ...), empty for all. */
  prefix: string;
  /** Hours a release must exist before it is announced (soak time), 0 for none. */
  soakHours: number;
}

export type WatchAction =
  | { kind: 'open-location' }
  | { kind: 'download'; folder: string; verify: boolean }
  | { kind: 'run-command'; program: string; args: string[] };

/** A watch as edited and stored (`spotter_core::watch::WatchSpec`). */
export interface WatchDraft {
  /** `null` for a new watch (the backend assigns one). */
  id: string | null;
  name: string;
  source: { kind: string; values: Record<string, string> };
  versions: VersionSchemeId;
  versionPattern: string | null;
  includePrereleases: boolean;
  /** Glob on release names; empty for all. */
  nameFilter: string;
  intervalSecs: number;
  notify: NotificationPolicy;
  actions: WatchAction[];
  channels: NotificationChannel[];
  filter: VersionFilter;
  paused: boolean;
}

/** One row of the watch list, the tiles and the tray. */
export interface WatchSummary {
  id: string;
  name: string;
  /** Source kind id (`folder`, `github`). */
  sourceKind: string;
  /** Source kind label (`GitHub`). */
  sourceLabel: string;
  /** Where it looks (`github.com/owner/repo`, `\\server\drops`). */
  sourceDescription: string;
  status: WatchStatus;
  paused: boolean;
  /** A check is running now. */
  checking: boolean;
  latest: Release | null;
  acknowledged: string | null;
  installed: string | null;
  /** The latest is newer than the acknowledged version. */
  unseen: boolean;
  /** `null` when the installed version is unknown. */
  newerThanInstalled: boolean | null;
  lastCheckMs: number | null;
  nextCheckMs: number | null;
  failures: number;
  intervalSecs: number;
}

export interface CheckRecord {
  atMs: number;
  tookMs: number;
  error: string | null;
  found: number;
}

/** The watch detail page. */
export interface WatchDetail {
  summary: WatchSummary;
  spec: WatchDraft;
  /** Newest first. */
  history: Release[];
  /** Newest last. */
  checks: CheckRecord[];
  /** Keys of secret fields that have a value in the credential store (values are never sent to the UI). */
  secretsSet: string[];
}

export type FieldKind = 'text' | 'url' | 'folder' | 'secret' | 'integer' | 'toggle' | 'choice';

/** One field of a source kind's form (`spotter_core::schema::FieldSpec`). */
export interface FieldSpec {
  key: string;
  label: string;
  help: string;
  kind: FieldKind;
  required: boolean;
  default: string;
  placeholder: string;
  /** For integers. */
  min: number | null;
  max: number | null;
  /** For choices. */
  options: { value: string; label: string }[];
}

export type SourceGroup = 'files' | 'web' | 'forge' | 'registry';

/** A built-in kind of source. The add-watch form is rendered from `fields`. */
export interface SourceKind {
  id: string;
  label: string;
  description: string;
  group: SourceGroup;
  fields: FieldSpec[];
}

/** The live "test this source" result. */
export interface SourcePreview {
  /** Problems with the draft (nothing was checked when there are any). */
  problems: string[];
  /** The check's failure, when it failed. */
  error: WatchStatus | null;
  /** Newest first, at most 20. */
  releases: Release[];
  /** Names that matched but held no version for the chosen scheme (up to 20). */
  withoutVersion: string[];
  prereleasesSkipped: number;
  namesSkipped: number;
  tookMs: number;
}

/** A ready-made watch for well-known software (a declarative pack). */
export interface Preset {
  id: string;
  name: string;
  description: string;
  category: string;
  /** The draft to start from; the user can still edit it. */
  draft: WatchDraft;
}

/** A feed found on a web page (auto-discovery). */
export interface DiscoveredFeed {
  url: string;
  title: string;
}

/** Which browser renders JavaScript pages (ADR 0002). */
export type RendererKind = 'chromium-installed' | 'chromium-downloaded' | 'os-webview' | 'none';

export interface RendererStatus {
  kind: RendererKind;
  /** Browser name and version, or why none is available and what to do. */
  detail: string;
  /** A pinned Chromium can be downloaded on request. */
  canDownload: boolean;
}

export type ImportFormat = 'winget' | 'brewfile' | 'opml';

export interface ImportResult {
  /** Drafts built from the list, not yet saved. */
  drafts: WatchDraft[];
  skipped: string[];
}

export type DashboardRange = '30d' | '90d' | '1y';

export interface WhatsNewItem {
  watchId: string;
  watchName: string;
  sourceKind: string;
  release: Release;
  releasedAtMs: number;
  newerThanInstalled: boolean | null;
  unseen: boolean;
}

export interface WatchNumbers {
  watchId: string;
  averageIntervalMs: number | null;
  sinceLastMs: number | null;
  /** Per bucket of `Dashboard.bucketsMs`. */
  releases: number[];
  failedChecks: number[];
  checks: number[];
  /** Releases per week over the last 12 weeks. */
  sparkline: number[];
}

export interface DownloadBucket {
  count: number;
  failed: number;
  bytes: number;
}

/** The dashboard (`spotter_core::dashboard`). */
export interface Dashboard {
  range: DashboardRange;
  /** Bucket start times, oldest first (days for 30d/90d, weeks for 1y). */
  bucketsMs: number[];
  whatsNew: WhatsNewItem[];
  watches: WatchNumbers[];
  downloads: DownloadBucket[];
  /** How long the engine took to compute this, in microseconds (the dashboard's own cost). */
  computeMicros: number;
}

export type AppEventKind = 'new-release' | 'check-failed' | 'recovered' | 'download-finished' | 'action-failed' | 'needs-you';

/** One entry of the events feed. Fields not used by a kind are null. */
export interface AppEvent {
  atMs: number;
  kind: AppEventKind;
  watchId: string | null;
  watchName: string | null;
  version: string | null;
  name: string | null;
  message: string | null;
  path: string | null;
  size: number | null;
  verified: boolean | null;
  action: string | null;
}

export interface Settings {
  theme: Theme;
  notifications: boolean;
  notificationsPausedUntilMs: number | null;
  checkAppUpdates: boolean;
  downloadsFolder: string | null;
  defaultIntervalSecs: number;
  /** 'off' announces at once; otherwise releases are collected into one notification. */
  digest: 'off' | 'daily' | 'weekly';
  /** Global quiet hours; per-watch quiet hours add to these. */
  quietHours: QuietHours | null;
  /** Use an OS webview when no Chromium browser is found (labelled as such). */
  allowOsWebview: boolean;
}

export interface AppInfo {
  version: string;
  configPath: string;
  logsDir: string;
  downloadsDir: string;
  /** Problems found while loading the configuration and state (worked around). */
  warnings: string[];
  /** The old Release Monitor's configuration was migrated at this start. */
  migratedFromLegacy: boolean;
  platform: 'windows' | 'macos' | 'linux';
}

export interface AppUpdateStatus {
  /** False in builds without the update key. */
  available: boolean;
  reason: string | null;
  currentVersion: string;
}

export interface AppUpdateCheck {
  updateAvailable: boolean;
  version: string | null;
  notes: string | null;
}

export type OpenTarget = 'config' | 'logs' | 'downloads';

/** A command error (`app/src-tauri/src/dto.rs` `CommandError`). */
export interface CommandError {
  code: 'badRequest' | 'notFound' | 'invalid' | 'unavailable' | 'internal';
  message: string;
  /** For `invalid`: every problem with the input. */
  problems: string[];
}

/** Pushed by the backend (Tauri event `spotter://state`) whenever watches change, checks start or end. */
export interface StatePush {
  watches: WatchSummary[];
}
