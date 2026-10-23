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

/** One file of a release (`spotter_core::release::Artifact`). */
export interface Artifact {
  name: string;
  location: string;
  /** Lower-case extension (`iso`, `exe`, `tar.gz`). */
  kind: string;
  /** `x64`, `lite`, ... or empty. */
  variant: string;
  size: number | null;
  modifiedMs: number | null;
  /** Null where the source does not report creation times. */
  createdMs: number | null;
  /** The checksum file next to it, by name. */
  checksumFile: string | null;
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
  /** The product (`id` group of the pattern); null for a watch of one thing. */
  product: string | null;
  /** The channel label as written (`beta`), empty for none. */
  channel: string;
  /** Other named groups of the pattern, for display. */
  fields: Record<string, string>;
  /** Every file of the release, the primary one first. */
  artifacts: Artifact[];
  /** The same file name came back with new contents. */
  updatedInPlace: boolean;
  /** A command that installs or pulls this release (container tags, package versions); Spotter never runs it. Always sent by the engine; null for none. */
  upgradeCommand?: string | null;
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
  | { kind: 'open-location'; variant?: string; extension?: string }
  | { kind: 'download'; folder: string; verify: boolean; variant?: string; extension?: string }
  | { kind: 'run-command'; program: string; args: string[] };

/** How the newest release is decided (`spotter_core::release::LatestBy`). */
export type LatestBy = 'version' | 'modified' | 'created' | 'name';

export interface ProductSetting {
  /** Display name; empty for the id itself. */
  name: string;
  muted: boolean;
  /** Own soak time; null uses the watch's. */
  soakHours: number | null;
}

/** Which release is the latest, and which products are tracked (`spotter_core::tracking::Tracking`). */
export interface Tracking {
  latestBy: LatestBy;
  /** A number group the name leaves out counts as older than any number (default: as zero). */
  missingOlder: boolean;
  products: {
    /** Track only these ids; empty for every product found. */
    only: string[];
    /** Announce a product that appears after the first check (off: it is a new baseline). */
    announceNew: boolean;
    settings: Record<string, ProductSetting>;
  };
}

export type Weekday = 'mon' | 'tue' | 'wed' | 'thu' | 'fri' | 'sat' | 'sun';

/** When a watch is checked (`spotter_core::schedule::Schedule`): every so often, at times of day, or a cron expression. */
export type Schedule =
  | { mode: 'every'; secs: number }
  /** `times` are `HH:MM`; `zone` is an IANA name, or `null` for the computer's own. */
  | { mode: 'times'; days: Weekday[]; times: string[]; zone: string | null }
  | { mode: 'cron'; expr: string; zone: string | null };

export type ScheduleMode = Schedule['mode'];

/** A schedule in words with its next runs (computed in Rust; nothing is saved). */
export interface SchedulePreview {
  description: string;
  /** The next runs (at most 5). */
  nextRunsMs: number[];
  /** For example a schedule that is clamped to the source's minimum interval. */
  warnings: string[];
  /** The time zone the times are in. */
  zone: string;
}

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
  schedule: Schedule;
  notify: NotificationPolicy;
  actions: WatchAction[];
  channels: NotificationChannel[];
  filter: VersionFilter;
  paused: boolean;
  tracking: Tracking;
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
  /** When the watch is checked, in words (computed in Rust). */
  scheduleText: string;
  /** How many products the watch tracks (0 for a watch of one thing). */
  productCount: number;
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
  /** The products of a watch whose pattern has an `id` group. */
  products: Product[];
}

/** One product of a watch: its own latest release, new state and history. */
export interface Product {
  id: string;
  /** Renamed or the id. */
  name: string;
  muted: boolean;
  latest: Release | null;
  unseen: boolean;
  /** Newest first. */
  history: Release[];
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
  /** Shown under "More options". */
  advanced: boolean;
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
  /** What the pattern read from the names; null when nothing was found. */
  extraction: ExtractionPreview | null;
}

/** Where a named group matched, in UTF-16 code units (what a JavaScript string indexes by). */
export interface GroupSpan {
  group: string;
  start: number;
  end: number;
}

/** What was read from one name. */
export interface ReadResult {
  id: string | null;
  version: string;
  channel: string;
  variant: string;
  prerelease: boolean;
  fields: Record<string, string>;
  spans: GroupSpan[];
}

/** One row of the wizard's table of names. */
export interface NameRow {
  /** The text the pattern ran on (name, folder name or relative path). */
  text: string;
  fileName: string;
  size: number | null;
  modifiedMs: number | null;
  /** What was read; null when the name was ignored. */
  read: ReadResult | null;
  /** Why it was ignored (`ignored: no match: ...`). */
  ignored: string | null;
}

export interface VersionPreview {
  version: string;
  /** Would be the latest for its product. */
  latest: boolean;
  prerelease: boolean;
  artifacts: Artifact[];
}

export interface ProductPreview {
  /** Null for a watch of one thing. */
  id: string | null;
  /** The watch tracks it (no product list, or it is on the list). */
  tracked: boolean;
  /** In the chosen order, newest first. */
  versions: VersionPreview[];
}

/** How a folder scan went. */
export interface ScanStats {
  scanned: number;
  matched: number;
  folders: number;
  tookMs: number;
  /** The previous listing was reused because no folder changed. */
  reused: boolean;
  /** Files still changing (left out until they settle). */
  settling: number;
}

/** What the wizard shows about the names a source holds (`spotter_engine::view::ExtractionPreview`). */
export interface ExtractionPreview {
  /** Matches first, then a sample of the ignored names (at most 300 rows). */
  rows: NameRow[];
  products: ProductPreview[];
  scan: ScanStats | null;
  /** Names left out before or by the pattern. */
  ignoredCount: number;
  /** The pattern has an `id` group. */
  hasId: boolean;
  /** The source reports creation times (needed for "latest by created time"). */
  createdAvailable: boolean;
  /** How the latest is decided, in words (the tooltip). */
  orderNote: string;
  warnings: string[];
}

/** The result of trying a pattern on one name (the Test field). */
export interface PatternTest {
  /** The pattern is not valid: what to fix. */
  error: string | null;
  read: ReadResult | null;
  ignored: string | null;
}

/** A part of an example name the user selected for the pattern helper. */
export interface PatternSelection {
  role: 'id' | 'major' | 'minor' | 'patch' | 'build' | 'channel' | 'variant';
  /** First UTF-16 code unit. */
  start: number;
  /** One past the last. */
  end: number;
}

/** A pattern to start from (a declarative pack scheme). */
export interface SchemePreset {
  id: string;
  name: string;
  pattern: string;
  /** A name it reads. */
  example: string;
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
  /** The mail server for e-mail channels (its password is set with `setMailPassword`). Always sent by the engine; null for none. */
  smtp?: SmtpSettings | null;
}

/** How the connection to the mail server is secured: STARTTLS (port 587), TLS from the first byte (465), or none (trusted networks only). */
export type SmtpSecurity = 'start-tls' | 'tls' | 'none';

export interface SmtpSettings {
  host: string;
  port: number;
  security: SmtpSecurity;
  /** The account to sign in with; null or empty for a relay that needs none. */
  username: string | null;
  /** The sender address. */
  from: string;
}

/** Whether e-mail can be sent. The password itself is never returned. */
export interface MailStatus {
  configured: boolean;
  passwordSet: boolean;
}

/** A picture for a watch or catalogue entry. A null avatar means: draw the monogram. */
export interface Avatar {
  /** `remote`: from the watched site (cached); `bundled`: Spotter's mark for the source kind. */
  source: 'remote' | 'bundled';
  mime: string;
  /** The image as a `data:` address (the content security policy allows it). */
  dataUrl: string;
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
