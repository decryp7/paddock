import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { dialogsAnswering, fakeChartProvider } from '../../../testing/render';
import { AppsPage } from './apps-page';

async function settle(fixture: { detectChanges(): void; whenStable(): Promise<unknown> }) {
  for (let i = 0; i < 4; i++) {
    fixture.detectChanges();
    await fixture.whenStable();
    await new Promise((r) => setTimeout(r, 0));
  }
  fixture.detectChanges();
}

/** How the fake dialog windows answer, by dialog id (`undefined`: dismissed). */
let answers: Record<string, unknown> = {};

async function render(backend: PitwallBackend = new MockBackend()) {
  const dialogs = dialogsAnswering((id) => answers[id]);
  TestBed.configureTestingModule({ imports: [AppsPage], providers: [{ provide: BACKEND, useValue: backend }, fakeChartProvider, dialogs] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
  const fixture = TestBed.createComponent(AppsPage);
  await settle(fixture);
  const el = fixture.nativeElement as HTMLElement;
  const button = (root: ParentNode, text: string) => Array.from(root.querySelectorAll('button')).find((b) => b.textContent?.trim().startsWith(text)) as HTMLButtonElement | undefined;
  return { fixture, el, live, button, opened: dialogs.opened };
}

describe('AppsPage', () => {
  beforeEach(() => (answers = { 'pd.confirm': true, 'pw.key-secret': true }));

  it('lists apps, shows the first one with usage, keys and permissions', async () => {
    const { el } = await render();
    const items = el.querySelectorAll('.apps .app');
    expect(items.length).toBe(5);
    expect(items[0]!.getAttribute('aria-current')).toBe('true');
    expect(el.querySelector('.hero h2')?.textContent).toContain('Debrief');
    expect(el.querySelector('.usage')?.textContent).toContain('1,284');
    expect(el.querySelector('table.keys')?.textContent).toContain('pw_');
    expect(el.querySelector('pw-permissions-editor')).not.toBeNull();
  });

  it('selects another app', async () => {
    const { fixture, el } = await render();
    (el.querySelectorAll('.apps .app')[4] as HTMLButtonElement).click();
    await settle(fixture);
    expect(el.querySelector('.hero h2')?.textContent).toContain('Nightly report script');
    expect(el.querySelectorAll('table.keys tbody tr').length).toBe(2);
  });

  it('shows a new key once, in its own window, and never on the page', async () => {
    const { fixture, el, button, opened } = await render();
    button(el, 'Create key')!.click();
    await settle(fixture);
    const shown = opened.at(-1)!;
    expect(shown.id).toBe('pw.key-secret');
    expect(shown.inputs).toMatchObject({ appName: 'Debrief', secret: expect.stringMatching(/^pw_/), prefix: expect.any(String) });
    expect(el.querySelectorAll('table.keys tbody tr').length).toBe(2);
    expect(el.textContent).not.toContain(shown.inputs['secret'] as string);
  });

  it('revokes a key after confirming', async () => {
    const { fixture, el, opened } = await render();
    // Revoke is only in the key's menu, in danger text.
    expect(el.querySelector('button[aria-label^="Revoke key"]')).toBeNull();
    (el.querySelector('button[aria-label^="More actions for key"]') as HTMLButtonElement).click();
    await settle(fixture);
    const revoke = Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-panel .mat-mdc-menu-item')).find((b) => b.textContent!.includes('Revoke key'))!;
    expect(revoke.classList.contains('danger')).toBe(true);
    revoke.click();
    await settle(fixture);
    expect(opened.at(-1)).toMatchObject({ id: 'pd.confirm', inputs: { message: expect.stringContaining("can't be undone"), confirm: { label: 'Revoke key', destructive: true } } });
    expect(el.querySelector('table.keys')).toBeNull();
    expect(el.textContent).toContain('No keys');
  });

  it('opens the key menu on a right-click of its row', async () => {
    const { fixture, el } = await render();
    const row = el.querySelector('table.keys tbody tr') as HTMLElement;
    const items = () => Array.from(document.querySelectorAll('.mat-mdc-menu-panel .mat-mdc-menu-item')).map((b) => b.textContent!.trim());
    row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await settle(fixture);
    expect(items()).toEqual(['Copy key prefix', 'Revoke key…']);
  });

  it('opens the key menu from the keyboard: Shift+F10 on the focused row', async () => {
    const { fixture, el } = await render();
    const row = el.querySelector('table.keys tbody tr') as HTMLElement;
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    await settle(fixture);
    expect(Array.from(document.querySelectorAll('.mat-mdc-menu-panel .mat-mdc-menu-item')).map((b) => b.textContent!.trim())).toEqual(['Copy key prefix', 'Revoke key…']);
  });

  it('the header has one filled primary: New app, unless a pairing waits (then Approve is the primary)', async () => {
    const { el } = await render();
    const header = el.querySelector('.page-head') as HTMLElement;
    expect(header.querySelectorAll('.mat-mdc-unelevated-button').length).toBe(0);
    expect(el.querySelector('.pair-list .mat-mdc-unelevated-button')?.textContent).toContain('Approve');
    expect(el.querySelector('[class*="btn"]')).toBeNull();
  });

  it('New app is the filled primary when no pairing waits', async () => {
    const { el } = await render(new MockBackend({ empty: true }));
    expect(el.querySelector('.page-head .mat-mdc-unelevated-button')?.textContent).toContain('New app');
  });

  it('creates an app with the name its window answers, then shows its first key', async () => {
    answers['pw.create-app'] = 'Raycast';
    const { fixture, el, button, opened } = await render();
    button(el, 'New app')!.click();
    await settle(fixture);
    expect(opened.map((o) => o.id)).toEqual(['pw.create-app', 'pw.key-secret']);
    expect(el.querySelector('.hero h2')?.textContent).toContain('Raycast');
    expect(opened[1]!.inputs['appName']).toBe('Raycast');
  });

  it('creates nothing when New app is cancelled', async () => {
    const backend = new MockBackend();
    const spy = vi.spyOn(backend, 'createApp');
    const { fixture, el, button, opened } = await render(backend);
    button(el, 'New app')!.click();
    await settle(fixture);
    expect(opened.map((o) => o.id)).toEqual(['pw.create-app']);
    expect(spy).not.toHaveBeenCalled();
  });

  it('deletes an app after confirming', async () => {
    const { fixture, el, button } = await render();
    // Delete is in the "..." menu of the app, not a button on the page.
    expect(button(el, 'Delete app')).toBeUndefined();
    (el.querySelector('button[aria-label="More actions for Debrief"]') as HTMLButtonElement).click();
    await settle(fixture);
    Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-panel .mat-mdc-menu-item')).find((b) => b.textContent!.includes('Delete app'))!.click();
    await settle(fixture);
    expect(el.querySelectorAll('.apps .app').length).toBe(4);
    expect(el.querySelector('.hero h2')?.textContent).not.toContain('Debrief');
  });

  it('approves a pairing request and lists the new app', async () => {
    const { fixture, el, button } = await render();
    expect(el.querySelector('.pair-list')?.textContent).toContain('482 913');
    button(el.querySelector('.pair-list')!, 'Approve')!.click();
    await settle(fixture);
    expect(el.querySelector('.pair-list')).toBeNull();
    expect(el.querySelector('.apps')?.textContent).toContain('Obsidian Copilot');
  });

  it('shows an error with Retry when apps fail to load', async () => {
    const { fixture, el, button } = await render(new MockBackend({ failing: ['apps'] }));
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('not responding');
    button(el, 'Retry')!.click();
    await settle(fixture);
    expect(el.querySelector('[role="alert"]')).not.toBeNull();
  });

  it('shows an empty state without apps', async () => {
    const { el } = await render(new MockBackend({ empty: true }));
    expect(el.textContent).toContain('No apps yet');
  });

  describe('caching', () => {
    it('shows the cache figures of the selected app and whether its answers are cached', async () => {
      const { el } = await render();
      const panel = Array.from(el.querySelectorAll('pw-panel')).find((p) => p.textContent!.includes('Cached answers'))!;
      expect(panel.textContent).toContain('on for this app');
      const figures = Array.from(panel.querySelectorAll('.cache-stats div')).map((d) => d.textContent!.trim());
      expect(figures).toEqual(['Cache hits212', 'Misses388', 'Hit rate35 %', 'Prompt tokens reused1.2M']);
    });

    it('shows a dash for the hit rate and says how to turn caching on for an app that has none', async () => {
      const { el, fixture } = await render();
      (el.querySelectorAll('.apps .app')[1] as HTMLButtonElement).click();
      await settle(fixture);
      const panel = Array.from(el.querySelectorAll('pw-panel')).find((p) => p.textContent!.includes('Cached answers'))!;
      expect(panel.textContent).toContain('off for this app');
      expect(panel.textContent).toContain('Turn on Cache answers under Permissions');
      expect(panel.querySelectorAll('.cache-stats dd')[2]!.textContent).toBe('—');
    });

    it('saves the Cache answers toggle with the permissions', async () => {
      const backend = new MockBackend();
      const { el, fixture, button } = await render(backend);
      (el.querySelectorAll('.apps .app')[1] as HTMLButtonElement).click();
      await settle(fixture);
      const toggle = el.querySelector<HTMLButtonElement>('button[aria-label="Cache answers"]')!;
      expect(toggle.getAttribute('aria-checked')).toBe('false');
      toggle.click();
      await settle(fixture);
      button(el, 'Save')!.click();
      await settle(fixture);
      expect((await backend.apps()).find((a) => a.id === 'pitcrew')!.permissions.cacheResponses).toBe(true);
    });

    it("clears one app's cached answers after confirming", async () => {
      const backend = new MockBackend();
      const spy = vi.spyOn(backend, 'clearCache');
      const { el, fixture, button, opened } = await render(backend);
      button(el, "Clear this app's cached answers")!.click();
      await settle(fixture);
      expect(opened.at(-1)!.inputs).toMatchObject({ heading: "Clear Debrief's cached answers?", confirm: { label: 'Clear cached answers' } });
      expect(spy).toHaveBeenCalledWith({ kind: 'app', appId: 'debrief' });
    });

    it('keeps the answers when the confirmation is cancelled', async () => {
      const backend = new MockBackend();
      const spy = vi.spyOn(backend, 'clearCache');
      answers['pd.confirm'] = undefined;
      const { el, fixture, button } = await render(backend);
      button(el, "Clear this app's cached answers")!.click();
      await settle(fixture);
      expect(spy).not.toHaveBeenCalled();
    });
  });
});
