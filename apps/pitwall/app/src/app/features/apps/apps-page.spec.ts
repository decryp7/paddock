import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { fakeChartProvider } from '../../../testing/render';
import { AppsPage } from './apps-page';

async function settle(fixture: { detectChanges(): void; whenStable(): Promise<unknown> }) {
  for (let i = 0; i < 4; i++) {
    fixture.detectChanges();
    await fixture.whenStable();
    await new Promise((r) => setTimeout(r, 0));
  }
  fixture.detectChanges();
}

async function render(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ imports: [AppsPage], providers: [{ provide: BACKEND, useValue: backend }, fakeChartProvider] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
  const fixture = TestBed.createComponent(AppsPage);
  await settle(fixture);
  const el = fixture.nativeElement as HTMLElement;
  const button = (root: ParentNode, text: string) => Array.from(root.querySelectorAll('button')).find((b) => b.textContent?.trim().startsWith(text)) as HTMLButtonElement | undefined;
  return { fixture, el, live, button };
}

const overlay = () => document.querySelector('.cdk-overlay-container') as HTMLElement;

describe('AppsPage', () => {
  afterEach(() => overlay()?.replaceChildren());

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

  it('shows a new key once, in a dialog with Copy and a warning', async () => {
    const { fixture, el, button } = await render();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    button(el, 'Create key')!.click();
    await settle(fixture);
    const dialog = overlay().querySelector('pw-key-secret-dialog') as HTMLElement;
    expect(dialog.textContent).toContain("you won't see it again");
    const secret = (dialog.querySelector('#pw-key-secret') as HTMLInputElement).value;
    expect(secret).toMatch(/^pw_/);
    button(dialog, 'Copy')!.click();
    await settle(fixture);
    expect(writeText).toHaveBeenCalledWith(secret);
    expect(dialog.textContent).toContain('Copied');
    expect(el.querySelectorAll('table.keys tbody tr').length).toBe(2);
    expect(el.textContent).not.toContain(secret);
    button(dialog, "I've saved it")!.click();
    await settle(fixture);
    expect(overlay().querySelector('pw-key-secret-dialog')).toBeNull();
  });

  it('tells the user to copy by hand when the clipboard is unavailable', async () => {
    const { fixture, el, button } = await render();
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('denied')) }, configurable: true });
    button(el, 'Create key')!.click();
    await settle(fixture);
    const dialog = overlay().querySelector('pw-key-secret-dialog') as HTMLElement;
    button(dialog, 'Copy')!.click();
    await settle(fixture);
    expect(dialog.textContent).toContain('Ctrl+C');
  });

  it('revokes a key after confirming', async () => {
    const { fixture, el, button } = await render();
    // Revoke is only in the key's menu, in danger text.
    expect(el.querySelector('button[aria-label^="Revoke key"]')).toBeNull();
    (el.querySelector('button[aria-label^="More actions for key"]') as HTMLButtonElement).click();
    await settle(fixture);
    const revoke = Array.from(document.querySelectorAll<HTMLButtonElement>('.pw-menu .pw-menu-item')).find((b) => b.textContent!.includes('Revoke key'))!;
    expect(revoke.classList.contains('danger')).toBe(true);
    revoke.click();
    await settle(fixture);
    const confirm = overlay().querySelector('pw-confirm-dialog') as HTMLElement;
    expect(confirm.textContent).toContain("can't be undone");
    button(confirm, 'Revoke key')!.click();
    await settle(fixture);
    expect(el.querySelector('table.keys')).toBeNull();
    expect(el.textContent).toContain('No keys');
  });

  it('creates an app and then shows its first key', async () => {
    const { fixture, el, button } = await render();
    button(el, 'New app')!.click();
    await settle(fixture);
    const dlg = overlay().querySelector('pw-create-app-dialog') as HTMLElement;
    const create = button(dlg, 'Create app')!;
    expect(create.disabled).toBe(true);
    const input = dlg.querySelector('input') as HTMLInputElement;
    input.value = 'Raycast';
    input.dispatchEvent(new Event('input'));
    await settle(fixture);
    create.click();
    await settle(fixture);
    expect(el.querySelector('.hero h2')?.textContent).toContain('Raycast');
    expect(overlay().querySelector('pw-key-secret-dialog')?.textContent).toContain('Raycast');
  });

  it('deletes an app after confirming', async () => {
    const { fixture, el, button } = await render();
    // Delete is in the "..." menu of the app, not a button on the page.
    expect(button(el, 'Delete app')).toBeUndefined();
    (el.querySelector('button[aria-label="More actions for Debrief"]') as HTMLButtonElement).click();
    await settle(fixture);
    Array.from(document.querySelectorAll<HTMLButtonElement>('.pw-menu .pw-menu-item')).find((b) => b.textContent!.includes('Delete app'))!.click();
    await settle(fixture);
    button(overlay().querySelector('pw-confirm-dialog')!, 'Delete app')!.click();
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
});
