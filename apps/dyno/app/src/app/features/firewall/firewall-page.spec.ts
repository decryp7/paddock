import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import type { FirewallSource } from '../../core/api.types';
import { provideIcons } from '../../shared/icons';
import { nextFrame } from '../../testing/frame';
import { FirewallPage, rulesLine, sourceName } from './firewall-page';

const NOW = 1_700_000_000_000;

async function open(fail: string[] | ((m: MockBackend) => void) = []) {
  const mock = new MockBackend({ latencyMs: 0, now: () => NOW, fail: new Set(Array.isArray(fail) ? fail : []) });
  if (typeof fail === 'function') fail(mock);
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
    expect(alf.querySelectorAll('tbody tr')).toHaveLength(3);
    expect(alf.textContent).toContain('Block');
  });

  it('says a source the helper reads needs the helper, with the button, and shows no rules for it', async () => {
    const { el } = await open();
    const pf = el.querySelector('[data-source="pf"]')!;
    expect(pf.querySelector('[data-testid="reading-source-needs"]')!.textContent).toContain('pf firewall');
    expect(pf.querySelector('[data-testid="reading-source-turn-on"]')).not.toBeNull();
    expect(pf.querySelector('table')).toBeNull();
  });

  it('shows the chip and the rules of a source the helper read, and counts only for a caller who may not see them', async () => {
    const { el } = await open((m) => {
      const v = m.firewall.bind(m);
      m.firewall = async () => {
        const view = await v();
        view.sources[1] = { ...view.sources[1]!, via: 'helper', ruleCount: 12, rulesHidden: true, note: null };
        return view;
      };
    });
    const pf = el.querySelector('[data-source="pf"]')!;
    expect(pf.querySelector('[data-testid="reading-source-helper"]')).not.toBeNull();
    expect(pf.querySelector('[data-testid="counts-only"]')!.textContent).toContain('12 rules; only the owner or an administrator sees them.');
    expect(pf.querySelector('table')).toBeNull();
  });

  it('puts the state and the switches of a source on one row of chips', async () => {
    const { el } = await open();
    const rows = el.querySelectorAll('[data-source="applicationFirewall"] .states');
    expect(rows).toHaveLength(1);
    expect(rows[0]!.querySelectorAll('.chip')).toHaveLength(3);
  });

  it('is read-only: the only controls are Refresh and the way to turn on the helper; none changes a rule', async () => {
    const { el } = await open();
    const buttons = [...el.querySelectorAll('button')].map((b) => b.textContent!.trim());
    expect(buttons).toEqual(['Refresh', 'Turn on administrator readings']);
  });

  it('shows an error with Retry when the firewall cannot be read, and no Refresh beside it', async () => {
    const { el } = await open(['firewall']);
    expect(el.querySelector('[role="alert"]')!.textContent).toContain('The firewall could not be read');
    expect([...el.querySelectorAll('button')].map((b) => b.textContent!.trim())).toEqual(['Retry']);
  });

  it('words the names and the counts', () => {
    expect(sourceName('windowsFirewall')).toBe('Windows Defender Firewall');
    const s = { ruleCount: 5120, rules: new Array(4096).fill(null), truncated: 1024 } as unknown as FirewallSource;
    expect(rulesLine(s)).toBe('Showing 4,096 of 5,120 rules.');
    expect(rulesLine({ ruleCount: 1, rules: [], truncated: 0 } as unknown as FirewallSource)).toBe('1 rule.');
  });
});
