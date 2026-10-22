import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../core/mock-backend';
import { appProviders, byText, makeMock, settle, text } from '../../testing/test-helpers';
import { NOW } from '../../testing/test-helpers';
import { ActivityPage } from './activity/activity-page';
import { RunnerTile } from './dashboard/runner-tile';
import { RunnerDetailPage } from './runners/runner-detail-page';
import { RunnersPage } from './runners/runners-page';
import { DefaultsSection } from './settings/defaults-section';

describe('leftover processes in the UI', () => {
  let mock: MockBackend;
  beforeEach(() => {
    localStorage.clear();
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
  });
  afterEach(() => mock.dispose());

  it('the runner tile links to the Processes tab only when there are leftovers', async () => {
    const runners = (await mock.status()).runners;
    const mount = async (name: string) => {
      const f = TestBed.createComponent(RunnerTile);
      f.componentRef.setInput('runner', runners.find((r) => r.name === name));
      f.componentRef.setInput('now', NOW);
      await settle(f);
      return f.nativeElement as HTMLElement;
    };
    const chip = (await mount('build-win')).querySelector('a.leftover-chip') as HTMLAnchorElement;
    expect(text(chip)).toBe('3 leftover processes');
    expect(chip.getAttribute('href')).toContain('tab=processes');
    expect((await mount('ui-tests')).querySelector('.leftover-chip')).toBeNull();
  });

  it('the runners page shows the chip, with a singular form', async () => {
    const f = TestBed.createComponent(RunnersPage);
    await settle(f, 6);
    const el = f.nativeElement as HTMLElement;
    expect(el.querySelectorAll('.leftover-chip')).toHaveLength(1);
    expect(text(el.querySelector('.leftover-chip'))).toBe('3 leftover processes');
    await mock.stopProcess('build-win', 12408, null, false);
    await mock.stopProcess('build-win', 13370, null, false);
    await settle(f, 6);
    expect(text(el.querySelector('.leftover-chip'))).toBe('1 leftover process');
  });

  it('the runner page has Processes and Configuration tabs and opens them', async () => {
    await mock.status();
    const f = TestBed.createComponent(RunnerDetailPage);
    f.componentRef.setInput('name', 'build-win');
    await settle(f, 6);
    const el = f.nativeElement as HTMLElement;
    expect(Array.from(el.querySelectorAll('[role="tab"]')).map((t) => text(t))).toEqual(['Overview', 'Updates', 'Processes', 'Configuration', 'Disk & cleanup']);
    expect(text(el.querySelector('.leftover-chip'))).toBe('3 leftover processes');
    f.componentRef.setInput('tab', 'processes');
    await settle(f, 6);
    expect(el.querySelector('pc-processes-panel')).not.toBeNull();
    f.componentRef.setInput('tab', 'config');
    await settle(f, 6);
    expect(el.querySelector('pc-config-panel')).not.toBeNull();
  });

  it('Activity shows peak CPU and memory and what a job left running, and hides zeros', async () => {
    const f = TestBed.createComponent(ActivityPage);
    await settle(f, 8);
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('thead'))).toContain('Peak CPU');
    expect(text(el.querySelector('thead'))).toContain('Peak memory');
    expect(text(el.querySelector('thead'))).toContain('Left running');
    const rows = Array.from(el.querySelectorAll('tbody tr'));
    expect(rows.some((r) => /left running/.test(text(r)))).toBe(true);
    expect(rows.some((r) => !/left running/.test(text(r)))).toBe(true);
    expect(rows[0]!.querySelectorAll("td")[6]!.textContent).toMatch(/%/);
    const page = f.componentInstance as unknown as { cpu(p: number): string; bytes(n: number): string };
    expect(page.cpu(420)).toBe('42%');
    expect(page.bytes(1024 ** 3)).toBe('1.0 GB');
  });

  it('Settings > Defaults edits and saves the leftover process policy', async () => {
    const f = TestBed.createComponent(DefaultsSection);
    await settle(f, 6);
    const el = f.nativeElement as HTMLElement;
    expect(text(el.querySelector('#processes-title'))).toBe('Leftover processes');
    const keep = el.querySelector('#defaults-processes-keep') as HTMLInputElement;
    keep.value = '20';
    keep.dispatchEvent(new Event('change', { bubbles: true }));
    f.detectChanges();
    const section = el.querySelector('pc-process-policy-editor') as HTMLElement;
    byText<HTMLButtonElement>(section, 'button', /^Save$/)!.click();
    await settle(f, 6);
    expect((await mock.settings()).config.defaults.processes.keep).toEqual({ amount: 20, unit: 'minutes' });
  });

  it('Saving the other defaults keeps the process policy', async () => {
    const f = TestBed.createComponent(DefaultsSection);
    await settle(f, 6);
    const el = f.nativeElement as HTMLElement;
    const field = el.querySelector('#stall-threshold') as HTMLInputElement;
    field.value = '8';
    field.dispatchEvent(new Event('change', { bubbles: true }));
    f.detectChanges();
    byText<HTMLButtonElement>(el, 'button', 'Save defaults')!.click();
    await settle(f, 6);
    const d = (await mock.settings()).config.defaults;
    expect(d.watchdog.threshold).toBe(8);
    expect(d.processes.rules).toHaveLength(5);
  });
});
