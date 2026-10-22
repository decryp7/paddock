import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import type { PitcrewBackend } from '../core/backend';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { ICONS } from '../shared/icons';
import { StatusView } from './status-view';

async function render(backend: PitcrewBackend) {
  TestBed.configureTestingModule({ imports: [StatusView], providers: [{ provide: BACKEND, useValue: backend }] });
  const registry = TestBed.inject(MatIconRegistry);
  const sanitizer = TestBed.inject(DomSanitizer);
  for (const [name, svg] of Object.entries(ICONS)) registry.addSvgIconLiteralInNamespace('pc', name, sanitizer.bypassSecurityTrustHtml(svg));
  const fixture = TestBed.createComponent(StatusView);
  fixture.detectChanges();
  await fixture.whenStable();
  fixture.detectChanges();
  return fixture.nativeElement as HTMLElement;
}

describe('StatusView', () => {
  it('lists every runner with its status, version and last update', async () => {
    const el = await render(new MockBackend());
    const items = el.querySelectorAll('li.runner');
    expect(items.length).toBe(2);
    expect(items[0]?.textContent).toContain('build-pc');
    expect(items[0]?.textContent).toContain('Running a job');
    expect(items[0]?.textContent).toContain('Updated 5.1.0 → 5.2.0');
    expect(items[1]?.textContent).toContain('5.2.0 available');
    expect(items[1]?.querySelector('.status')?.getAttribute('data-tone')).toBe('warn');
  });

  it('shows an error with Retry, and Retry loads again', async () => {
    let calls = 0;
    const backend: PitcrewBackend = {
      status: () => {
        calls++;
        return calls === 1 ? Promise.reject({ code: 'serviceUnavailable', message: 'The Pitcrew service is not running.' }) : Promise.resolve([]);
      },
      appUpdateStatus: () => Promise.reject(new Error('unused')),
    };
    const el = await render(backend);
    const alert = el.querySelector('[role="alert"]');
    expect(alert?.textContent).toContain('The Pitcrew service is not running.');
    const retry = Array.from(el.querySelectorAll('button')).find((b) => b.textContent?.includes('Retry'));
    retry?.click();
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toBe(2);
  });

  it('has an accessible refresh button', async () => {
    const el = await render(new MockBackend([]));
    expect(el.querySelector('button[aria-label="Refresh"]')).not.toBeNull();
    expect(el.textContent).toContain('No runners yet');
  });
});
