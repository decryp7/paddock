import { provideChannelsBackend, provideShellChannels } from '@decryp7/paddock-ui/shell/channels';
import { demoChannelsBackend } from './demo-channels';

/** What the Notification channels page boots with (loaded with the page, never in the main window's first paint). */
export const CHANNELS_PROVIDERS = [provideShellChannels({ backend: demoChannelsBackend })];

/** What the secret dialog's window boots with: the backend only (the secret is never state). */
export const CHANNELS_SECRET_PROVIDERS = [provideChannelsBackend({ backend: demoChannelsBackend })];
