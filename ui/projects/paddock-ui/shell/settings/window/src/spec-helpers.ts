// Test helpers of the settings specs (not exported from the entry point).
import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { defineSettingsPage, msg, type SettingsPageDef } from '@decryp7/paddock-ui/shell';
import { MemorySettingsBackend, type SettingsBackend } from '@decryp7/paddock-ui/shell/settings';
import { settingsWindowProviders } from './provide-settings';

/** The shell demo's pages: General (application) and Storage (features); Appearance is the shell's. */
export const SPEC_PAGES: readonly SettingsPageDef[] = [
  defineSettingsPage({ id: 'sd.general', group: 'application', title: msg('sd.settings.general', 'General'), order: 100 }),
  defineSettingsPage({ id: 'sd.storage', group: 'features', title: msg('sd.settings.storage', 'Storage'), order: 100 }),
];

/** Lets promises, resources and effects settle (zoneless). */
export async function settle(): Promise<void> {
  for (let i = 0; i < 4; i++) {
    for (let j = 0; j < 8; j++) await Promise.resolve();
    TestBed.tick();
  }
}

/** TestBed with a Settings window's providers over a memory engine (the fixtures Rust wrote). */
export function settingsBed(
  options: { backend?: SettingsBackend; pages?: readonly SettingsPageDef[]; extra?: unknown[]; appearance?: boolean; restart?: () => void } = {},
): { backend: SettingsBackend } {
  const backend = options.backend ?? new MemorySettingsBackend();
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      ...settingsWindowProviders({
        backend: () => backend,
        pages: options.pages ?? SPEC_PAGES,
        appearance: options.appearance ?? false,
        ...(options.restart ? { restart: () => options.restart as () => void } : {}),
      }),
      ...((options.extra ?? []) as never[]),
    ],
  });
  return { backend };
}
