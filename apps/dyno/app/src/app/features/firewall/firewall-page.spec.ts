import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import type { FirewallSource } from '../../core/api.types';
import { provideIcons } from '../../shared/icons';
import { nextFrame } from '../../testing/frame';
import { FirewallPage, rulesLine, sourceName } from './firewall-page';

const NOW = 1_700_000_000_000;

async function open(fail: string[] = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(fail) });
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(FirewallPage);
  f.detectChanges();
  for (let i = 0; i < 4; i++) {
    await f.whenStable();
    await nextFrame();
    f.detectChanges();
  }
  return { f, el: f.nativeElement as HTMLElement };
}

afterEach(() => TestBed.resetTestingModule());

describe('FirewallPage', () => {
  it('shows each source with its state in words, its switches and its rules', async () => {
    const { el } = await open();
    const alf = el.querySelector('[data-source="applicationFirewall"]')!;
    expect(alf.textContent).toContain('macOS Application Firewall');
    expect(alf.textContent).toContain('Firewall: On');
    expect(alf.textContent).toContain('Stealth mode: On');
    expect(alf.textContent).toContain('Block all incoming: Off');
    expect(alf.querySelectorAll('tbody tr')).toHaveLength(2);
    expect(alf.textContent).toContain('Block');
  });

  it('says a source the helper reads needs the helper, and shows no rules for it', async () => {
    const { el } = await open();
    const pf = el.querySelector('[data-source="pf"]')!;
    expect(pf.querySelector('[data-testid="source-note"]')!.textContent).toContain('need the helper');
    expect(pf.querySelector('table')).toBeNull();
  });

  it('is read-only: the only controls are Refresh and a way to the helper setting; none changes a rule', async () => {
    const { el } = await open();
    const buttons = [...el.querySelectorAll('button')].map((b) => b.textContent!.trim());
    expect(buttons).toEqual(['Refresh', 'Open settings']);
  });

  it('shows an error with Retry when the firewall cannot be read, and no Refresh beside it', async () => {
    const { el } = await open(['firewall']);
    expect(el.querySelector('[role="alert"]')!.textContent).toContain('firewall');
    expect([...el.querySelectorAll('button')].map((b) => b.textContent!.trim())).toEqual(['Retry']);
  });

  it('words the names and the counts', () => {
    expect(sourceName('windowsFirewall')).toBe('Windows Defender Firewall');
    const s = { ruleCount: 5120, rules: new Array(4096).fill(null), truncated: 1024 } as unknown as FirewallSource;
    expect(rulesLine(s)).toBe('Showing 4,096 of 5,120 rules.');
    expect(rulesLine({ ruleCount: 1, rules: [], truncated: 0 } as unknown as FirewallSource)).toBe('1 rule.');
  });
});
