import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { RequestsPage, msCell } from './requests-page';
import { RequestsStore } from './requests.store';

async function settle(fixture: { detectChanges(): void; whenStable(): Promise<unknown> }) {
  for (let i = 0; i < 3; i++) {
    fixture.detectChanges();
    await fixture.whenStable();
    await new Promise((r) => setTimeout(r, 0));
  }
  fixture.detectChanges();
}

async function render(backend: PitwallBackend = new MockBackend()) {
  TestBed.configureTestingModule({ imports: [RequestsPage], providers: [{ provide: BACKEND, useValue: backend }] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const fixture = TestBed.createComponent(RequestsPage);
  await settle(fixture);
  const store = fixture.debugElement.injector.get(RequestsStore);
  return { fixture, el: fixture.nativeElement as HTMLElement, store };
}

describe('msCell', () => {
  it('formats table milliseconds compactly', () => {
    expect(msCell(null)).toBe('—');
    expect(msCell(212.4)).toBe('212');
    expect(msCell(1460)).toBe('1.5 s');
    expect(msCell(12_400)).toBe('12 s');
  });
});

describe('RequestsPage', () => {
  it('renders the filter row and the table header', async () => {
    const { el, store } = await render();
    expect(el.querySelector('[role="search"]')).not.toBeNull();
    expect(el.querySelectorAll('[role="columnheader"]').length).toBe(12);
    expect(el.querySelector('.shown')?.textContent).toContain(`${store.records().length} of ${store.records().length}`);
    const options = Array.from(el.querySelectorAll('#pw-req-app option')).map((o) => o.textContent?.trim());
    expect(options[0]).toBe('All apps');
    expect(options.length).toBeGreaterThan(2);
  });

  it('filters by text, app and status from the controls', async () => {
    const { fixture, el, store } = await render();
    const search = el.querySelector('#pw-req-search') as HTMLInputElement;
    search.value = 'embeddings';
    search.dispatchEvent(new Event('input'));
    const app = el.querySelector('#pw-req-app') as HTMLSelectElement;
    app.value = 'spotter';
    app.dispatchEvent(new Event('change'));
    (Array.from(el.querySelectorAll('[role="radio"]')).find((b) => b.textContent?.includes('Errors')) as HTMLButtonElement).click();
    await settle(fixture);
    expect(store.filter()).toEqual({ app: 'spotter', status: 'errors', text: 'embeddings' });
    store.setFilter({ text: 'nothing-matches-this' });
    await settle(fixture);
    expect(el.textContent).toContain('No request matches');
  });

  it('opens the detail with content hidden, shows it on request, and closes on Escape', async () => {
    const { fixture, el, store } = await render();
    const r = store.records().find((x) => x.hasContent && x.status === 200)!;
    store.select(r.id);
    await settle(fixture);
    const drawer = el.querySelector('pw-request-detail') as HTMLElement;
    expect(drawer.textContent).toContain(r.endpoint);
    expect(drawer.querySelector('pre')).toBeNull();
    (Array.from(drawer.querySelectorAll('button')).find((b) => b.textContent?.includes('Show content')) as HTMLButtonElement).click();
    await settle(fixture);
    expect(drawer.querySelectorAll('pre').length).toBe(2);
    (Array.from(drawer.querySelectorAll('button')).find((b) => b.textContent?.includes('Hide content')) as HTMLButtonElement).click();
    await settle(fixture);
    expect(drawer.querySelector('pre')).toBeNull();
    drawer.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape' }));
    await settle(fixture);
    expect(el.querySelector('pw-request-detail')).toBeNull();
  });

  it('shows which preset an app asked for and what it resolved to', async () => {
    const { fixture, el, store } = await render();
    const withAlias = store.records().find((x) => x.alias !== null)!;
    store.select(withAlias.id);
    await settle(fixture);
    expect(el.querySelector('[role="row"][aria-label]')).not.toBeNull();
    expect(el.textContent).toContain(`via ${withAlias.alias}`);
    const route = (el.querySelector('pw-request-detail') as HTMLElement).querySelector('[aria-labelledby="pw-req-route"]')!.textContent!;
    expect(route).toContain('Preset asked for');
    expect(route).toContain(withAlias.alias!);
    expect(route).toContain('Resolved to');
    expect(route).toContain(withAlias.modelId!);
    const plain = store.records().find((x) => x.alias === null && x.modelId !== null)!;
    store.select(plain.id);
    await settle(fixture);
    const route2 = (el.querySelector('pw-request-detail') as HTMLElement).querySelector('[aria-labelledby="pw-req-route"]')!.textContent!;
    expect(route2).not.toContain('Preset asked for');
    expect(route2).toContain('Model');
  });

  it('explains when a request kept no content', async () => {
    const { fixture, el, store } = await render();
    const r = store.records().find((x) => !x.hasContent && x.error)!;
    store.select(r.id);
    await settle(fixture);
    const drawer = el.querySelector('pw-request-detail') as HTMLElement;
    expect(drawer.textContent).toContain("didn't keep");
    expect(drawer.textContent).toContain(r.error!);
    (drawer.querySelector('button[aria-label="Close details"]') as HTMLButtonElement).click();
    await settle(fixture);
    expect(el.querySelector('pw-request-detail')).toBeNull();
  });

  it('shows content errors with Retry', async () => {
    const backend = new MockBackend();
    const { fixture, el, store } = await render(backend);
    vi.spyOn(backend, 'requestContent').mockRejectedValue({ code: 'serverUnavailable', message: 'Server down' });
    store.select(store.records().find((x) => x.hasContent)!.id);
    await store.showContent();
    await settle(fixture);
    expect(el.querySelector('pw-request-detail [role="alert"]')?.textContent).toContain('Server down');
  });

  it('shows the load error with Retry, and an empty state without requests', async () => {
    const failing = await render(new MockBackend({ failing: ['requests'] }));
    expect(failing.el.querySelector('[role="alert"]')).not.toBeNull();
    TestBed.resetTestingModule();
    const empty = await render(new MockBackend({ empty: true }));
    expect(empty.el.textContent).toContain('No requests yet');
  });
});
