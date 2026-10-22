import type { AppPermissions, PairingRequest } from '../../core/api.types';
import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { BACKEND } from '../../core/backend';
import { LiveStore } from '../../core/live.store';
import { MockBackend } from '../../core/mock-backend';
import { registerIcons } from '../../shared/icons';
import { settle } from '../../../testing/render';

import { PairingCard, groupCode, permissionSummary } from './pairing-card';

const NOW = 1_800_000_000_000;
const perms = (over: Partial<AppPermissions> = {}): AppPermissions => ({
  models: null,
  maxPriority: 'interactive',
  tools: false,
  extendedApis: false,
  limits: { requestsPerMin: null, tokensPerDay: null, maxConcurrent: null, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null },
  ...over,
});

describe('groupCode', () => {
  it('splits a six digit code in two groups', () => expect(groupCode('482913')).toBe('482 913'));
  it('leaves other lengths alone', () => {
    expect(groupCode('1234')).toBe('1234');
    expect(groupCode('')).toBe('');
  });
});

describe('permissionSummary', () => {
  it('summarises the minimum', () => expect(permissionSummary(perms())).toEqual(['Every model', 'Up to interactive priority']));
  it('counts models with the right plural', () => {
    expect(permissionSummary(perms({ models: ['a'] }))[0]).toBe('1 model');
    expect(permissionSummary(perms({ models: ['a', 'b'] }))[0]).toBe('2 models');
    expect(permissionSummary(perms({ models: [] }))[0]).toBe('0 models');
  });
  it('lists tools, extended APIs and the rate limit when granted', () => {
    const s = permissionSummary(perms({ maxPriority: 'batch', tools: true, extendedApis: true, limits: { requestsPerMin: 60, tokensPerDay: null, maxConcurrent: null, requestsPerDay: null, tokensPerMonth: null, maxMemoryPercent: null } }));
    expect(s).toEqual(['Every model', 'Up to batch priority', 'Tools', 'Extended APIs', '60 requests/min']);
  });
});

const request: PairingRequest = { id: 'p1', appId: 'x', appName: 'Notes App', code: '123456', requestedAt: NOW - 5 * 60_000, permissions: perms({ tools: true }) };

async function mount(backend = new MockBackend({ now: () => NOW })) {
  TestBed.configureTestingModule({ imports: [PairingCard], providers: [{ provide: BACKEND, useValue: backend }] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const live = TestBed.inject(LiveStore);
  await live.load();
  const fixture = TestBed.createComponent(PairingCard);
  fixture.componentRef.setInput('request', request);
  fixture.componentRef.setInput('now', NOW);
  await settle(fixture);
  return { fixture, el: fixture.nativeElement as HTMLElement, live, backend };
}

describe('PairingCard', () => {
  it('shows who asks, the grouped code, the permissions and how long ago', async () => {
    const { el } = await mount();
    expect(el.querySelector('section')!.getAttribute('aria-label')).toBe('Pairing request from Notes App');
    expect(el.querySelector('.title')!.textContent).toContain('Notes App wants to use Pitwall');
    expect(el.querySelector('.digits')!.textContent).toBe('123 456');
    expect(el.querySelector('.sub')!.textContent).toContain('Asked 5 min ago');
    expect(Array.from(el.querySelectorAll('.perms li')).map((l) => l.textContent)).toEqual(['Every model', 'Up to interactive priority', 'Tools']);
  });

  it('approves with exactly the requested permissions and clears the request', async () => {
    const { fixture, el, backend, live } = await mount();
    const spy = vi.spyOn(backend, 'approvePairing');
    const approve = Array.from(el.querySelectorAll('button')).find((b) => b.textContent!.includes('Approve'))!;
    approve.click();
    fixture.detectChanges();
    expect(approve.disabled).toBe(true);
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('p1', request.permissions);
    expect(live.isBusy()('pairing:p1')).toBe(false);
  });

  it('denies and disables both buttons while in flight', async () => {
    const { fixture, el, backend } = await mount();
    const spy = vi.spyOn(backend, 'denyPairing');
    const [deny, approve] = Array.from(el.querySelectorAll('button')) as HTMLButtonElement[];
    deny!.click();
    fixture.detectChanges();
    expect(deny!.disabled && approve!.disabled).toBe(true);
    await settle(fixture);
    expect(spy).toHaveBeenCalledWith('p1');
    expect(deny!.disabled).toBe(false);
  });
});
