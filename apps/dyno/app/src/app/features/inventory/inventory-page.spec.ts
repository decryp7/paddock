import { TestBed } from '@angular/core/testing';
import type { SystemInventory } from '../../core/api.types';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { mockInventory } from '../../core/mock/inventory';
import { InventoryStore } from '../../state/inventory.store';
import { InventoryPage } from './inventory-page';

async function render(os: 'windows' | 'macos' | 'linux' | SystemInventory) {
  const backend = typeof os === 'string' ? new MockBackend({ latencyMs: 0, os }) : ({ inventory: () => Promise.resolve(os) } as never);
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: backend }] });
  const f = TestBed.createComponent(InventoryPage);
  f.detectChanges();
  await TestBed.inject(InventoryStore).load();
  f.detectChanges();
  return f;
}

describe('InventoryPage', () => {
  it('draws the slots with a text alternative, hints and a table view', async () => {
    const f = await render('windows');
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelector('svg[role=img]')!.getAttribute('aria-label')).toContain('2 of 4 memory slots filled');
    expect(el.querySelectorAll('.hints li').length).toBe(4);
    expect(el.textContent).toContain('Board maximum');
    [...el.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Table')!.click();
    f.detectChanges();
    await f.whenStable();
    await new Promise<void>((r) => requestAnimationFrame(() => r()));
    f.detectChanges();
    expect(el.querySelectorAll('pd-table [role="row"][data-key]')).toHaveLength(4);
    expect(el.textContent).toContain('CMK16GX4M1E3200C16');
  });

  it('shows Apple silicon unified memory as part of the chip', async () => {
    const f = await render('macos');
    const el = f.nativeElement as HTMLElement;
    expect(el.textContent).toContain('cannot be upgraded');
    expect(el.querySelector('svg[role=img]')!.getAttribute('aria-label')).toContain('Unified memory');
    expect(el.querySelectorAll('dy-device-art')).toHaveLength(2);
  });

  it('says why the modules are unknown when only the total is available', async () => {
    const inv: SystemInventory = { ...mockInventory('windows'), memory: { totalOnly: { total: 16 * 1024 ** 3, reason: { reason: 'The OS did not answer.' } } }, hints: [] };
    const f = await render(inv);
    const el = f.nativeElement as HTMLElement;
    expect(el.textContent).toContain('16 GB installed');
    expect(el.textContent).not.toContain('helper');
  });

  it('shows an inline error with Retry', async () => {
    TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: { inventory: () => Promise.reject({ code: 'unsupported', message: 'not built yet' }) } }] });
    const f = TestBed.createComponent(InventoryPage);
    f.detectChanges();
    await TestBed.inject(InventoryStore).load();
    f.detectChanges();
    expect(f.nativeElement.textContent).toContain('not built yet');
    expect(TestBed.inject(InventoryStore).error()?.code).toBe('unsupported');
  });
});
