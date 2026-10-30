import { TestBed } from '@angular/core/testing';
import { aboutBackend, BrowserEngines, BROWSER_VERSION, notifyBackends, RELEASES_PAGE, settingsBackend, updatesBackend } from './engines';
import { tauriBridge, tauriIpc } from './ipc';

describe('the browser build’s stand-ins for the shell’s Rust parts', () => {
  it('outside Tauri there is no IPC or bridge', () => {
    expect(tauriIpc()).toBeNull();
    expect(tauriBridge()).toBeNull();
  });

  it('About names Pitwall, its release page and the three legal documents, and is one engine per page', async () => {
    const about = aboutBackend();
    expect(aboutBackend()).toBe(about);
    const info = await about.info();
    expect(info).toMatchObject({ appName: 'Pitwall', version: BROWSER_VERSION, releasesPage: RELEASES_PAGE });
    expect(info.legal.map((l) => l.id)).toEqual(['notices', 'licence-mit', 'licence-apache']);
    expect(updatesBackend()).toBe(updatesBackend());
  });

  it('settings show the shell’s Appearance only, health lists no service and the centre is shared', () => {
    TestBed.runInInjectionContext(() => {
      const engines = TestBed.inject(BrowserEngines);
      expect(settingsBackend()).toBe(engines.settings);
      expect(notifyBackends().health()).toBe(engines.health);
      expect(notifyBackends().notifications()).toBe(engines.centre);
    });
  });
});
