import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Toasts } from '../../core/toasts';
import { registerIcons } from '../../shared/icons';
import { PluginsPage } from './plugins-page';
import { PluginsStore } from './plugins.store';

function setupStore(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ providers: [PluginsStore, { provide: BACKEND, useValue: backend }] });
  return { store: TestBed.inject(PluginsStore), toasts: TestBed.inject(Toasts) };
}

describe('PluginsStore', () => {
  it('loads plugins and MCP servers', async () => {
    const { store } = setupStore();
    await store.load();
    expect(store.plugins().length).toBe(3);
    expect(store.mcp().length).toBe(2);
    expect(store.pluginsError()).toBeNull();
  });

  it('keeps one list when only the other fails', async () => {
    const { store } = setupStore(new MockBackend({ failing: ['mcpServers'] }));
    await store.load();
    expect(store.plugins().length).toBe(3);
    expect(store.mcpError()?.code).toBe('serverUnavailable');
  });

  it('toggles a plugin and refreshes its state', async () => {
    const { store, toasts } = setupStore();
    await store.load();
    const done = store.setEnabled('pw-web-fetch', true);
    expect(store.plugins().find((p) => p.id === 'pw-web-fetch')!.enabled).toBe(true);
    expect(store.isBusy('pw-web-fetch')).toBe(true);
    await store.setEnabled('pw-web-fetch', true);
    await done;
    expect(store.plugins().find((p) => p.id === 'pw-web-fetch')!.state).toBe('running');
    expect(store.isBusy('pw-web-fetch')).toBe(false);
    expect(toasts.toasts().at(-1)?.message).toContain('enabled');
  });

  it('rolls back a refused toggle with a toast', async () => {
    const backend = new MockBackend();
    const { store, toasts } = setupStore(backend);
    await store.load();
    vi.spyOn(backend, 'setPluginEnabled').mockRejectedValue({ code: 'forbidden', message: 'Not allowed' });
    await store.setEnabled('pw-pii-filter', false);
    expect(store.plugins().find((p) => p.id === 'pw-pii-filter')!.enabled).toBe(true);
    expect(toasts.toasts().at(-1)).toMatchObject({ tone: 'bad', message: 'Not allowed' });
  });

  it('keeps a successful toggle when the refresh fails', async () => {
    const backend = new MockBackend();
    const { store } = setupStore(backend);
    await store.load();
    vi.spyOn(backend, 'plugins').mockRejectedValue(new Error('gone'));
    await store.setEnabled('pw-pii-filter', false);
    expect(store.plugins().find((p) => p.id === 'pw-pii-filter')!.enabled).toBe(false);
  });
});

async function render(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ imports: [PluginsPage], providers: [{ provide: BACKEND, useValue: backend }] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const fixture = TestBed.createComponent(PluginsPage);
  for (let i = 0; i < 3; i++) {
    fixture.detectChanges();
    await fixture.whenStable();
  }
  fixture.detectChanges();
  return fixture.nativeElement as HTMLElement;
}

describe('PluginsPage', () => {
  it('shows plugins with kinds, state, network and quarantine reason, and MCP servers', async () => {
    const el = await render();
    const items = el.querySelectorAll('.plugins > li');
    expect(items.length).toBe(3);
    expect(items[0]!.textContent).toContain('Network: never');
    expect(items[2]!.textContent).toContain('Quarantined');
    expect(items[2]!.textContent).toContain('Timed out after 30 s');
    expect(items[2]!.textContent).toContain('3 failures');
    expect(el.querySelectorAll('mat-slide-toggle').length).toBe(3);
    const mcp = el.querySelectorAll('.mcp > li');
    expect(mcp.length).toBe(2);
    expect(mcp[0]!.textContent).toContain('read_file');
    expect(mcp[1]!.textContent).toContain('No app may use it yet');
  });

  it('shows empty states', async () => {
    const el = await render(new MockBackend({ empty: true }));
    expect(el.textContent).toContain('No plugins installed');
    expect(el.textContent).toContain('No MCP servers');
  });

  it('shows each list error with Retry', async () => {
    const el = await render(new MockBackend({ failing: ['plugins', 'mcpServers'] }));
    expect(el.querySelectorAll('[role="alert"]').length).toBe(2);
  });
});
