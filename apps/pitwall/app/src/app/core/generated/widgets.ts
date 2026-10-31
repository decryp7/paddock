// Generated from Pitwall's widget wire types (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p pitwall-core --lib widgets::generated.

export type PitwallSurface = "popover";

export type PopoverWidget = "server" | "activity" | "memory" | "loadedModels" | "downloads" | "updates" | "recentErrors";

export type LoadingRows = { 
/**
 * The widget kind.
 */
kind: PopoverWidget, 
/**
 * How many placeholder rows its loading twin draws.
 */
rows: number, };

export type ServerState = "serving" | "paused" | "stopped" | "unreachable";

export type EngineHealth = { "kind": "ready" } | { "kind": "installing" } | { "kind": "missing" } | { "kind": "restarting", 
/**
 * How many models.
 */
count: number, } | { "kind": "failed", 
/**
 * How many models.
 */
count: number, };

export type MeterTone = "neutral" | "warn" | "bad";

export type MemoryLevel = "normal" | "elevated" | "critical";

export type VramReading = { 
/**
 * Bytes in use.
 */
used: number, 
/**
 * Bytes in all.
 */
total: number, 
/**
 * The share in use, 0 to 100.
 */
percent: number, 
/**
 * How busy the GPU is, 0 to 100, when it reports it.
 */
busyPercent: number | null, 
/**
 * The meter's tone.
 */
tone: MeterTone, };

export type ModelRowState = "ready" | "busy" | "loading" | "releasing" | "failed";

export type ModelRow = { 
/**
 * The model id.
 */
id: string, 
/**
 * Its name.
 */
name: string, 
/**
 * Memory it holds (RAM and video memory), in bytes.
 */
bytes: number, 
/**
 * What it is doing.
 */
state: ModelRowState, 
/**
 * Kept loaded.
 */
pinned: boolean, 
/**
 * Requests it is answering.
 */
active: number, };

export type DownloadRowState = "paused" | "failed";

export type DownloadRow = { 
/**
 * The download id.
 */
id: string, 
/**
 * What is downloaded.
 */
name: string, 
/**
 * Paused or failed.
 */
state: DownloadRowState, 
/**
 * Why it failed, when it did.
 */
message: string | null, };

export type ErrorRow = { 
/**
 * A stable id (the event's).
 */
id: string, 
/**
 * One sentence (never request content).
 */
message: string, 
/**
 * When, in milliseconds since the epoch.
 */
atMs: number, };

export type PopoverStatus = { "kind": "unknown" } | { "kind": "unreachable" } | { "kind": "stopped" } | { "kind": "paused" } | { "kind": "problems", 
/**
 * How many models' engines crashed.
 */
count: number, } | { "kind": "serving", 
/**
 * The loopback port.
 */
port: number, };

export type ServingToggle = "pause" | "resume" | "off";

export type PopoverHeader = { 
/**
 * How the local AI is doing overall.
 */
status: PopoverStatus, 
/**
 * What Pause serving or Resume serving does.
 */
serving: ServingToggle, 
/**
 * Whether Release memory has anything to release.
 */
canReleaseMemory: boolean, };

export type Page = "dashboard" | "models" | "engines" | "requests";

export type PopoverValue = { "kind": "server", 
/**
 * Whether it answers.
 */
state: ServerState, 
/**
 * The loopback port; zero when unknown.
 */
port: number, 
/**
 * The engines; none while nothing is known.
 */
engine: EngineHealth | null, } | { "kind": "activity", 
/**
 * Requests running.
 */
running: number, 
/**
 * Requests waiting.
 */
waiting: number, 
/**
 * Generated tokens per second, rounded; zero while serving is paused.
 */
tokensPerSec: number, } | { "kind": "memory", 
/**
 * RAM in use, in bytes.
 */
used: number, 
/**
 * RAM in all, in bytes.
 */
total: number, 
/**
 * The share in use, 0 to 100.
 */
percent: number, 
/**
 * The meter's tone.
 */
tone: MeterTone, 
/**
 * The OS's pressure.
 */
pressure: MemoryLevel, 
/**
 * Video memory, when a GPU reports it.
 */
vram: VramReading | null, } | { "kind": "loadedModels", 
/**
 * All loaded models.
 */
total: number, 
/**
 * The busiest first, then the most recently used.
 */
rows: Array<ModelRow>, 
/**
 * Models not listed.
 */
more: number, 
/**
 * Memory all of them hold, in bytes.
 */
bytes: number, } | { "kind": "downloads", 
/**
 * The paused and failed downloads, failed first.
 */
rows: Array<DownloadRow>, 
/**
 * Not listed.
 */
more: number, } | { "kind": "updates", 
/**
 * How many.
 */
count: number, 
/**
 * The first one's sentence.
 */
summary: string | null, } | { "kind": "recentErrors", 
/**
 * The listed errors.
 */
rows: Array<ErrorRow>, 
/**
 * Errors of the last day not listed.
 */
more: number, };

export type WidgetActionId = { "type": "openPage", 
/**
 * Which.
 */
page: Page, } | { "type": "cancelDownload", 
/**
 * The download id.
 */
id: string, };
