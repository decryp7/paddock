import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitwallBackend } from '../../core/backend';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { COLUMNS, RequestsPage, layout, msCell } from './requests-page';
import { secondsCell, statusText, tokensCell } from './request-labels';
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

describe('request cells', () => {
  it('words an error status, shows seconds and joins the tokens', () => {
    expect(statusText(429)).toBe('429 Rate limited');
    expect(statusText(418)).toBe('418');
    expect(secondsCell(null)).toBe('—');
    expect(secondsCell(143)).toBe('0.1');
    expect(secondsCell(2900)).toBe('2.9');
    expect(secondsCell(13_400)).toBe('13');
    expect(secondsCell(130_000)).toBe('130');
    expect(tokensCell(212, 1400)).toBe('212 / 1.4k');
  });

  it('lays out the default columns and adds the optional ones on request', () => {
    const base = layout(new Set());
    expect(base.cols.some((c) => c.optional)).toBe(false);
    expect(base.template.endsWith(' 32px')).toBe(true);
    const all = layout(new Set(COLUMNS.filter((c) => c.optional).map((c) => c.id)));
    expect(all.cols.length).toBe(COLUMNS.length);
    expect(base.narrow).not.toBe(base.template);
    expect(base.compact).toBe('68px minmax(88px, 160px) minmax(88px, 1fr) 72px 132px 32px');
    expect(base.tiny).toBe('68px minmax(100px, 1fr) 132px 32px');
  });
});

describe('RequestsPage', () => {
  it('shows nothing in the status column for a success and the status word for an error', async () => {
    const { fixture: f, el, store } = await render();
    const failed = store.records().find((r) => r.status >= 400)!;
    const ok = store.records().find((r) => r.status < 400)!;
    const rows = Array.from(el.querySelectorAll<HTMLElement>('[role="row"][aria-label]'));
    const rowOf = (id: string) => rows.find((r) => r.getAttribute('aria-label')!.includes(store.records().find((x) => x.id === id)!.endpoint));
    expect(rows.length).toBeGreaterThan(0);
    expect(el.querySelectorAll('.c-status .status').length).toBe(rows.filter((r) => r.classList.contains('failed')).length);
    expect(rowOf(ok.id)).toBeDefined();
    store.setFilter({ status: 'errors' });
    await settle(f);
    expect(el.querySelector('.c-status .status')?.textContent).toContain(String(failed.status));
  });

  it('adds a column from the Columns menu', async () => {
    const { fixture, el } = await render();
    el.querySelector<HTMLButtonElement>('button[aria-label="Columns"]')!.click();
    await settle(fixture);
    const item = Array.from(document.querySelectorAll<HTMLButtonElement>('[role="menuitemcheckbox"]')).find((b) => b.textContent!.includes('API'))!;
    item.click();
    await settle(fixture);
    expect(Array.from(el.querySelectorAll('[role="columnheader"]')).map((h) => h.textContent!.trim())).toContain('API');
    item.click();
    await settle(fixture);
    expect(Array.from(el.querySelectorAll('[role="columnheader"]')).map((h) => h.textContent!.trim())).not.toContain('API');
  });

  it('opens the details with Enter, and offers copy and an app filter in the row menu', async () => {
    const { fixture, el, store } = await render();
    const row = el.querySelector<HTMLElement>('[role="row"][tabindex]')!;
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true }));
    await settle(fixture);
    expect(store.selected()).not.toBeNull();
    const write = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: write } });
    row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await settle(fixture);
    const labels = Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-panel .mat-mdc-menu-item'));
    expect(labels.map((b) => b.textContent!.trim())).toEqual(['Details', 'Copy request id', 'Copy endpoint', expect.stringContaining('Only requests from')]);
    labels[1]!.click();
    await settle(fixture);
    expect(write).toHaveBeenCalledWith(store.visible()[0]!.id);
    row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await settle(fixture);
    Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-panel .mat-mdc-menu-item')).at(-1)!.click();
    await settle(fixture);
    expect(store.filter().app).not.toBe('all');
    vi.unstubAllGlobals();
  });

  it('opens the row menu from the keyboard (Shift+F10)', async () => {
    const { fixture, el } = await render();
    const row = el.querySelector<HTMLElement>('[role="row"][tabindex]')!;
    const items = () => Array.from(document.querySelectorAll('.mat-mdc-menu-panel .mat-mdc-menu-item')).map((b) => b.textContent!.trim());
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    await settle(fixture);
    expect(items()).toEqual(['Details', 'Copy request id', 'Copy endpoint', expect.stringContaining('Only requests from')]);
  });

  it('opens the same row menu from the row\'s "..." button', async () => {
    const { fixture, el } = await render();
    const row = el.querySelector<HTMLElement>('[role="row"][tabindex]')!;
    row.querySelector<HTMLButtonElement>('button[aria-label^="More actions for the request"]')!.click();
    await settle(fixture);
    expect(Array.from(document.querySelectorAll('.mat-mdc-menu-panel .mat-mdc-menu-item')).map((b) => b.textContent!.trim())).toEqual(['Details', 'Copy request id', 'Copy endpoint', expect.stringContaining('Only requests from')]);
  });

  it('filters by status with a Material segmented control that counts the errors', async () => {
    const { fixture, el, store } = await render();
    const group = el.querySelector('mat-button-toggle-group[aria-label="Status"]')!;
    const errors = Array.from(group.querySelectorAll<HTMLButtonElement>('[role="radio"]')).find((b) => b.textContent!.includes('Errors'))!;
    expect(errors.textContent).toContain(String(store.errorCount()));
    errors.click();
    await settle(fixture);
    expect(store.filter().status).toBe('errors');
  });

  it('renders the filter row and the table header', async () => {
    const { fixture, el, store } = await render();
    expect(el.querySelector('[role="search"]')).not.toBeNull();
    // Calm by default: the columns that repeat on every row (API, chosen by, queued, cold start) are off.
    expect(Array.from(el.querySelectorAll('[role="columnheader"]')).map((h) => h.textContent!.trim())).toEqual(['Time', 'App', 'Model', 'Endpoint', 'TTFT ms', 'Total s', 'Tokens in / out', 'Status', 'Actions']);
    expect(el.querySelector('.shown')?.textContent).toContain(`${store.records().length} requests`);
    expect(el.querySelector('select')).toBeNull();
    expect(el.querySelector('#pw-req-search')?.closest('mat-form-field')).not.toBeNull();
    el.querySelector<HTMLElement>('#pw-req-app')!.click();
    await settle(fixture);
    const options = Array.from(document.querySelectorAll('.cdk-overlay-container mat-option')).map((o) => o.textContent?.trim());
    expect(options[0]).toBe('All apps');
    expect(options.length).toBeGreaterThan(2);
  });

  it('filters by text, app and status from the controls', async () => {
    const { fixture, el, store } = await render();
    const search = el.querySelector('#pw-req-search') as HTMLInputElement;
    search.value = 'embeddings';
    search.dispatchEvent(new Event('input'));
    el.querySelector<HTMLElement>('#pw-req-app')!.click();
    await settle(fixture);
    Array.from(document.querySelectorAll<HTMLElement>('.cdk-overlay-container mat-option'))
      .find((o) => o.textContent!.trim().toLowerCase() === 'spotter')!
      .click();
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
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
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

describe('request caching', () => {
  const pick = (store: RequestsStore, outcome: 'hit' | 'miss' | 'bypass' | null) => store.records().find((x) => x.cache === outcome && x.status === 200)!;

  it('shows a cache hit chip with the X-Pitwall-Cache header in the detail', async () => {
    const { fixture, el, store } = await render();
    store.select(pick(store, 'hit').id);
    await settle(fixture);
    const drawer = el.querySelector('pw-request-detail') as HTMLElement;
    const chip = drawer.querySelector('.cache .status')!;
    expect(chip.textContent).toBe('Cache hit');
    expect(chip.getAttribute('data-tone')).toBe('ok');
    expect(drawer.querySelector('.cache')!.textContent).toContain('X-Pitwall-Cache');
    expect(drawer.querySelector('.cache')!.textContent).toContain('says hit');
  });

  it('words a miss and a bypass, and shows nothing for a request that was not eligible', async () => {
    const { fixture, el, store } = await render();
    store.select(pick(store, 'miss').id);
    await settle(fixture);
    expect(el.querySelector('pw-request-detail .cache')!.textContent).toContain('Cache miss');
    store.select(pick(store, 'bypass').id);
    await settle(fixture);
    expect(el.querySelector('pw-request-detail .cache')!.textContent).toContain('Cache bypassed');
    store.select(pick(store, null).id);
    await settle(fixture);
    expect(el.querySelector('pw-request-detail .cache')).toBeNull();
  });

  it('shows the prompt tokens reused only when there were some', async () => {
    const { fixture, el, store } = await render();
    store.select(store.records().find((x) => x.cachedTokens > 0)!.id);
    await settle(fixture);
    const route = () => (el.querySelector('pw-request-detail') as HTMLElement).querySelector('[aria-labelledby="pw-req-route"]')!.textContent!;
    expect(route()).toContain('Prompt tokens reused');
    store.select(store.records().find((x) => x.cachedTokens === 0)!.id);
    await settle(fixture);
    expect(route()).not.toContain('Prompt tokens reused');
  });

  it('marks cache hits in the list', async () => {
    const { el, store } = await render();
    expect(store.records().some((r) => r.cache === 'hit' && r.status < 400)).toBe(true);
    expect(el.querySelector('.cached')?.textContent).toContain('cached');
  });
});
});
