// Hand-written mirror of paddock-channels' Tauri adapter (plan X17; the Rust side is written next and must match these
// shapes): camelCase JSON, as the adapter sends it. The limits here are the engine's; the engine re-validates everything.

/** The event the engine sends the outcome of a test on. */
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

/** How serious an event must be before a channel hears of it. */
export type MinSeverity = 'info' | 'warning' | 'error';
/** The kinds of transport a channel can have. */
export type TransportType = 'webhook' | 'ntfy' | 'email';
/** How an email connection is secured. */
export type EmailSecurity = 'startTls' | 'implicit' | 'none';

/** Where a channel delivers to. `auth` says the channel needs a secret (token or password) that the engine keeps. */
export type Transport =
  | { readonly type: 'webhook'; readonly url: string; readonly auth: boolean }
  | { readonly type: 'ntfy'; readonly server: string; readonly topic: string; readonly auth: boolean }
  | {
      readonly type: 'email';
      readonly host: string;
      readonly port: number;
      readonly security: EmailSecurity;
      readonly username: string | null;
      readonly from: string;
      readonly to: readonly string[];
    };

/** One notification channel. */
export interface ChannelConfig {
  readonly id: string;
  readonly name: string;
  readonly enabled: boolean;
  /** The event names it hears of (the app's own, e.g. `diskLow`). */
  readonly events: readonly string[];
  readonly minSeverity: MinSeverity;
  readonly transport: Transport;
}

/** The channels the app keeps. */
export interface ChannelsSettings {
  readonly channels: readonly ChannelConfig[];
}

/** What `channels_get` and `channels_save` answer. `secretSet[id]` is true when the channel needs a secret and one is stored. */
export interface ChannelsView {
  readonly settings: ChannelsSettings;
  readonly secretSet: Readonly<Record<string, boolean>>;
}

export type ChannelsErrorCode = 'invalid' | 'storeUnavailable' | 'busy' | 'failed';

/** What a channels command rejects with. `message` is the engine's, shown to the user as it is. */
export interface ChannelsError {
  readonly code: ChannelsErrorCode;
  readonly message: string;
  readonly retryable: boolean;
}

/** The outcome of `channels_test`, sent on `paddock://channels` (`message` is the engine's advice on a failure). */
export interface ChannelsEvent {
  readonly kind: 'test';
  readonly testId: number;
  readonly channelId: string;
  readonly ok: boolean;
  readonly attempts: number;
  readonly message: string | null;
}

/** `channels_test` answers at once with the id its outcome will carry. */
export interface TestStarted {
  readonly testId: number;
}
