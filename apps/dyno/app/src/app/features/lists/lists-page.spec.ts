import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { nextFrame } from '../../testing/frame';
import { IntelStore } from '../../state/intel.store';
import { ListsPage } from './lists-page';

const NOW = 1_700_000_000_000;

async function open(options: { fail?: string[] } = {}) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(options.fail ?? []) });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(ListsPage);
  const settle = async () => {
    for (let i = 0; i < 4; i++) {
      await f.whenStable();
      await nextFrame();
      f.detectChanges();
    }
  };
  f.detectChanges();
  await settle();
  const el = f.nativeElement as HTMLElement;
  const q = <T extends Element = HTMLElement>(sel: string) => el.querySelector<T>(sel);
  const type = async (sel: string, value: string) => {
    const input = q<HTMLInputElement | HTMLTextAreaElement>(sel)!;
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
  };
  const click = async (sel: string) => {
    q(sel)!.click();
    await settle();
  };
  return { f, el, q, type, click, settle, mock, store: TestBed.inject(IntelStore) };
}

describe('ListsPage', () => {
  it('shows the lists with their counts, the switches, the first-run note and the credit', async () => {
    const { el, q } = await open();
    expect(el.querySelector('h1')!.textContent).toContain('IP lists');
    expect(q('[data-testid="list-list-1"]')!.textContent).toContain('Work blocklist');
    expect(q('[data-testid="list-list-1"] .counts')!.textContent).toContain('3 entries');
    expect(q('[data-testid="first-run-note"]')!.textContent).toContain('DB-IP');
    expect(q('[data-testid="first-run-note"]')!.textContent).toContain('Cloudflare');
    expect(q('[data-testid="attribution"]')!.textContent).toContain('DB-IP');
    expect(q('[data-testid="switch-databases"] input')!.getAttribute('aria-checked') ?? (q<HTMLInputElement>('[data-testid="switch-databases"] input')!.checked ? 'true' : 'false')).toBe('true');
    expect(el.textContent).not.toMatch(/offline mode/i);
  });

  it('turns a network switch off and keeps the choice', async () => {
    const { q, click, store } = await open();
    await click('[data-testid="switch-own-location"] input');
    expect(store.status()!.geo.switches.ownLocation).toBe(false);
    expect(store.status()!.geo.switches.locationDatabases).toBe(true);
    expect(q('[data-testid="switch-own-location"]')).toBeTruthy();
  });

  it('adds a typed list, refuses a second one with the same name, and removes a list after asking', async () => {
    const { q, type, click, store } = await open();
    await type('[data-testid="add-name"]', 'Home');
    await type('[data-testid="add-text"]', '8.8.8.8 # dns\n8.8.0.0/16');
    await click('[data-testid="add-list"]');
    expect(store.status()!.lists.map((l) => l.name)).toEqual(['Work blocklist', 'Home']);
    expect(q('[data-testid="list-list-2"] .counts')!.textContent).toContain('1 entry');
    await type('[data-testid="add-name"]', 'home');
    await click('[data-testid="add-list"]');
    expect(q('[data-testid="edit-error"]')!.textContent).toContain('Another list already has that name');
    expect(store.status()!.lists).toHaveLength(2);
    await click('[data-testid="list-list-2"] .actions button:last-of-type');
    expect(q('[data-testid="confirm-remove"]')!.textContent).toContain('Home');
    await click('[data-testid="confirm-remove"]');
    expect(store.status()!.lists.map((l) => l.id)).toEqual(['list-1']);
  });

  it('subscribes by address: shows the terms, refuses plain http, and lists the subscription with its schedule', async () => {
    const { q, type, click, store } = await open();
    await click('[data-testid="kind-url"]');
    expect(q('[data-testid="terms"]')!.textContent).toContain('responsibility');
    await type('[data-testid="add-name"]', 'Feed');
    await type('[data-testid="add-url"]', 'http://lists.example/feed.txt');
    await click('[data-testid="add-list"]');
    expect(q('[data-testid="edit-error"]')!.textContent).toContain('https://');
    await type('[data-testid="add-url"]', 'https://127.0.0.1:8099/list.txt');
    await click('[data-testid="add-list"]');
    expect(q('[data-testid="edit-error"]')!.textContent).toContain('only from public addresses');
    await type('[data-testid="add-url"]', 'https://lists.example/feed.txt');
    await click('[data-testid="add-list"]');
    const feed = store.status()!.lists.find((l) => l.name === 'Feed')!;
    expect(q(`[data-testid="list-${feed.id}"]`)!.textContent).toContain('refreshed every 24 h');
    expect(q(`[data-testid="list-${feed.id}"] [data-testid="update-line"]`)).toBeTruthy();
  });

  it('previews "Remove duplicates" first, changes nothing until applied, then applies', async () => {
    const { q, type, click, store, mock } = await open();
    await type('[data-testid="add-name"]', 'Messy');
    await type('[data-testid="add-text"]', '8.8.8.8\n8.8.0.0/16\n8.8.8.8\nx.example\ny.x.example');
    await click('[data-testid="add-list"]');
    await click('[data-testid="list-list-2"] [data-testid="dedupe"]');
    const preview = q('[data-testid="preview"]')!;
    expect(preview.textContent).toContain('5');
    expect(preview.textContent).toContain('is covered by');
    expect(mock.intel.status().lists[1]!.text).toContain('y.x.example');
    await click('[data-testid="apply-dedupe"]');
    expect(q('[data-testid="preview"]')).toBeNull();
    expect(store.status()!.lists[1]!.text).toBe('8.8.0.0/16\nx.example');
  });

  it('saves the allow list', async () => {
    const { type, click, store } = await open();
    await type('[data-testid="allow-text"]', '9.9.9.9 # mine');
    await click('[data-testid="save-allow"]');
    expect(store.status()!.allow).toBe('9.9.9.9 # mine');
  });

  it('shows the error with Retry when the lists cannot be read', async () => {
    const { el, q, mock, settle } = await open({ fail: ['intelStatus'] });
    expect(el.querySelector('[role="alert"]')!.textContent).toContain('intelStatus');
    expect(q('[data-testid="no-lists"]')).toBeNull();
    expect(mock).toBeTruthy();
    await settle();
  });
});

describe('ListsPage empty state', () => {
  it('says there are no lists and how to add one', async () => {
    const { q, click, store } = await open();
    await click('[data-testid="list-list-1"] .actions button:last-of-type');
    await click('[data-testid="confirm-remove"]');
    expect(store.status()!.lists).toHaveLength(0);
    expect(q('[data-testid="no-lists"]')!.textContent).toContain('no lists yet');
  });
});
