import { CHANNELS_EVENT, type ChannelConfig, type ChannelsEvent, type ChannelsSettings, type ChannelsView } from '../generated/channels';
import type { ChannelsCommand } from '../generated/channels-commands';
import { parseChannelsEvent, parseChannelsView, parseTestStarted } from './wire';

/** Stops listening. */
export type Unlisten = () => void;

/**
 * The app's IPC as this backend needs it, so this package does not depend on `@tauri-apps/api`: pass Tauri's `invoke` and an
 * adapter over its `listen` (`(event, cb) => listen(event, (e) => cb(e.payload))`).
 */
export interface ChannelsIpc {
  invoke<T>(command: ChannelsCommand, args?: Record<string, unknown>): Promise<T>;
  listen(event: string, handler: (payload: unknown) => void): Promise<Unlisten>;
}

/**
 * The notification channels' port (`paddock_channels::tauri`). The engine validates and acts; this only asks and shows. A test
 * returns at once with its id: the outcome (it can take a minute) arrives by `listen`. Every command may reject with a
 * `ChannelsError` whose message is for the user as it is.
 */
export abstract class ChannelsBackend {
  /** The channels and which of them have a secret stored. */
  abstract get(): Promise<ChannelsView>;
  /** Replaces the channels; rejects `invalid` with the engine's message. Answers the view as stored. */
  abstract save(settings: ChannelsSettings): Promise<ChannelsView>;
  /** Stores the secret (token or password) of channel `id`. Called from the secret dialog's window only. */
  abstract setSecret(id: string, value: string): Promise<void>;
  /** Deletes the stored secret of channel `id`. */
  abstract clearSecret(id: string): Promise<void>;
  /** Starts a test send with `channel` (it uses the stored secret); resolves with the id its outcome will carry. */
  abstract test(channel: ChannelConfig): Promise<number>;
  /** Every event the engine sends until the returned function is called. */
  abstract listen(handler: (event: ChannelsEvent) => void): Promise<Unlisten>;
}

/** The channels over Tauri. Payloads are parsed at this edge. */
export class TauriChannelsBackend extends ChannelsBackend {
  constructor(private readonly ipc: ChannelsIpc) {
    super();
  }

  async get(): Promise<ChannelsView> {
    return this.view(await this.ipc.invoke<unknown>('channels_get'));
  }

  async save(settings: ChannelsSettings): Promise<ChannelsView> {
    return this.view(await this.ipc.invoke<unknown>('channels_save', { settings }));
  }

  async setSecret(id: string, value: string): Promise<void> {
    await this.ipc.invoke<unknown>('channels_set_secret', { id, value });
  }

  async clearSecret(id: string): Promise<void> {
    await this.ipc.invoke<unknown>('channels_clear_secret', { id });
  }

  async test(channel: ChannelConfig): Promise<number> {
    const id = parseTestStarted(await this.ipc.invoke<unknown>('channels_test', { channel }));
    if (id === null) throw new Error('The engine did not say which test it started.');
    return id;
  }

  listen(handler: (event: ChannelsEvent) => void): Promise<Unlisten> {
    return this.ipc.listen(CHANNELS_EVENT, (payload) => {
      const event = parseChannelsEvent(payload);
      if (event) handler(event);
    });
  }

  private view(raw: unknown): ChannelsView {
    const view = parseChannelsView(raw);
    if (!view) throw new Error('The engine sent no valid notification channels.');
    return view;
  }
}
