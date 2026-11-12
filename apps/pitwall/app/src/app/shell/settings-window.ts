import type { EnvironmentProviders, Provider } from '@angular/core';
import type { SettingsPageDef } from '@decryp7/paddock-ui/shell/contributions';
import { settingsWindowProviders } from '@decryp7/paddock-ui/shell/settings/window';
import type { PitwallBackend } from '../core/backend';
import { createBackend } from '../core/backend-factory';
import { SETTINGS_PAGES } from './contributions';
import { settingsBackend } from './engines';

/** The chat's pages (its connected apps, its chats): only in a build with the chat. */
const CHAT_PAGES: readonly string[] = ['pw.connected-apps', 'pw.chats'];

/**
 * Pitwall's Settings pages for this build: Connected apps and Chats only when Rust says the chat is built in (`chat_enabled`, the
 * `chat` feature until ADR 0049's switches); a build that cannot say has no chat. The shared Settings window does not
 * evaluate a page's `when` (it boots without the app's context), so the list is decided here, before the window starts.
 */
export async function settingsPagesFor(backend: PitwallBackend): Promise<readonly SettingsPageDef[]> {
  const chat = await backend.chatEnabled().catch(() => false);
  return chat ? SETTINGS_PAGES : SETTINGS_PAGES.filter((p) => !CHAT_PAGES.includes(p.id));
}

/** The Settings window's providers (its dialog's `providers`): loaded with it, never in the main window's first paint. */
export async function settingsWindow(): Promise<(Provider | EnvironmentProviders)[]> {
  const pages = await settingsPagesFor(createBackend());
  return [...settingsWindowProviders({ backend: settingsBackend, pages: [...pages] })];
}
