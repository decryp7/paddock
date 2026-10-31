// Generated from the Rust wire types of paddock-channels (ts-rs). Do not edit: PADDOCK_WRITE_TS=1 cargo test -p paddock-channels --lib generated.

export type MinSeverity = "info" | "warning" | "error";

export type EmailSecurity = "startTls" | "implicit" | "none";

export type Transport = { "type": "webhook", 
/**
 * Target URL (https, or http for a service on the local network).
 */
url: string, 
/**
 * Send the stored token as `Authorization: Bearer <token>`.
 */
auth: boolean, } | { "type": "ntfy", 
/**
 * Server base URL.
 */
server: string, 
/**
 * Topic name (`A-Za-z0-9_-`).
 */
topic: string, 
/**
 * Send the stored access token as `Authorization: Bearer <token>`.
 */
auth: boolean, } | { "type": "email", 
/**
 * SMTP host.
 */
host: string, 
/**
 * SMTP port.
 */
port: number, 
/**
 * Connection security.
 */
security: EmailSecurity, 
/**
 * Login name; `None` for a relay that needs no login (then no password is used).
 */
username: string | null, 
/**
 * Sender address.
 */
from: string, 
/**
 * Recipient addresses.
 */
to: Array<string>, };

export type ChannelConfig = { 
/**
 * Unique id within the settings (`A-Za-z0-9._-`, at most 64): also the credential store scope.
 */
id: string, 
/**
 * Display name.
 */
name: string, 
/**
 * Off by default.
 */
enabled: boolean, 
/**
 * Event kinds this channel receives; empty means none.
 */
events: Array<string>, 
/**
 * Messages below this severity are skipped.
 */
minSeverity: MinSeverity, 
/**
 * The channel's server is on the user's home network: validation accepts a private address (and plain http to it), and the
 * poster reaches private networks for this channel only. Off by default; files from before this field load as off.
 */
allowLan: boolean, 
/**
 * Delivery details.
 */
transport: Transport, };

export type ChannelsSettings = { 
/**
 * Configured channels (at most [`MAX_CHANNELS`]).
 */
channels: Array<ChannelConfig>, };

export type ChannelsView = { 
/**
 * The saved settings.
 */
settings: ChannelsSettings, 
/**
 * Per channel id: a secret is stored (only channels that need one are listed).
 */
secretSet: { [key in string]?: boolean }, };

export type ChannelsErrorCode = "invalid" | "storeUnavailable" | "busy" | "failed";

export type ChannelsError = { 
/**
 * The kind.
 */
code: ChannelsErrorCode, 
/**
 * What to do, for the user.
 */
message: string, 
/**
 * Whether trying again can help.
 */
retryable: boolean, };

export type TestPhase = "sending" | "waiting";

export type TestStarted = { 
/**
 * Matches the `testId` of the events that report the test.
 */
testId: number, };

export type ChannelsEvent = { "kind": "testProgress", 
/**
 * The id [`ChannelsControl::test`] returned.
 */
testId: number, 
/**
 * The channel.
 */
channelId: string, 
/**
 * The attempt under way or just made (1 to `of`).
 */
attempt: number, 
/**
 * The most attempts the test makes.
 */
of: number, 
/**
 * What it is doing.
 */
phase: TestPhase, 
/**
 * How long the wait is, in seconds, while waiting.
 */
waitSecs: number | null, } | { "kind": "test", 
/**
 * The id [`ChannelsControl::test`] returned.
 */
testId: number, 
/**
 * The channel.
 */
channelId: string, 
/**
 * Delivered.
 */
ok: boolean, 
/**
 * Attempts made.
 */
attempts: number, 
/**
 * Why not, in words the user can act on.
 */
message: string | null, };

/** The kinds of transport a channel can have. */
export type TransportType = Transport['type'];

/** The event every update (test progress and outcome) is sent on, to every window. */
export const CHANNELS_EVENT = 'paddock://channels';

/** The most channels one app keeps. */
export const MAX_CHANNELS = 8;
/** The most event names one channel lists. */
export const MAX_EVENTS = 64;
/** The most recipients of an email channel. */
export const MAX_RECIPIENTS = 10;
/** A channel id: letters, digits, dot, underscore, hyphen; 1 to 64 characters. */
export const CHANNEL_ID_PATTERN = /^[A-Za-z0-9._-]{1,64}$/;
/** An ntfy topic: letters, digits, underscore, hyphen; 1 to 64 characters. */
export const NTFY_TOPIC_PATTERN = /^[A-Za-z0-9_-]{1,64}$/;
