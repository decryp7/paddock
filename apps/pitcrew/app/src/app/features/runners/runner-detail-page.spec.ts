import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { RunnerDetailPage } from './runner-detail-page';

describe('RunnerDetailPage resource use', () => {
  let mock: MockBackend;
  beforeEach(() => {
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
  });
  afterEach(() => mock.dispose());

  async function mount(name: string) {
    await mock.status();
    const f = TestBed.createComponent(RunnerDetailPage);
    f.componentRef.setInput('name', name);
    await settle(f, 6);
    return f.nativeElement as HTMLElement;
  }

  it('shows the runner and its processes: now, peaks and 24 h trends', async () => {
    const el = await mount('build-win');
    const card = el.querySelector('[aria-labelledby="use-title"]');
    expect(text(card)).toContain('this runner and every process it started');
    expect(text(card)).toContain('31%');
    expect(text(card)).toContain('4.2 GB');
    expect(text(card)).toContain('23');
    expect(card?.querySelectorAll('pc-usage-chart')).toHaveLength(2);
    expect(text(card)).toMatch(/CPU now, peak \d/);
    expect(text(card)).not.toContain('Memory now Processes');
  });

  it('draws the CPU chart from 0 to 100 % with a dashed peak line and its label', async () => {
    const el = await mount('build-win');
    const cpu = el.querySelector('.chart.cpu pc-usage-chart');
    expect(text(cpu)).toContain('100%');
    expect(text(cpu)).toMatch(/peak \d+(\.\d)?%/);
    expect(cpu?.querySelector('.peak-line')).not.toBeNull();
  });

  it('shows the hero numbers large, as captions above the charts', async () => {
    const el = await mount('build-win');
    const heroes = Array.from(el.querySelectorAll('.use .hero .big')).map((e) => text(e));
    expect(heroes).toEqual(['31%', '4.2 GB', '23']);
  });

  it('keeps one Restart button and moves Drain and Logs into the menu', async () => {
    const el = await mount('build-win');
    const header = el.querySelector('header.head') as HTMLElement;
    const labels = Array.from(header.querySelectorAll('button.btn, a.btn')).map((b) => text(b) || b.getAttribute('aria-label'));
    expect(labels).toEqual(['Restart', 'Actions for build-win']);
  });

  it('shows the status as a dot and words and the leftovers as one warning link', async () => {
    const el = await mount('build-win');
    const header = el.querySelector('header.head') as HTMLElement;
    expect(header.querySelector('pc-runner-status .dot')).not.toBeNull();
    expect(header.querySelector('.badge')).toBeNull();
    expect(text(header.querySelector('.leftover-link'))).toBe('3 leftover processes');
  });

  it('has five tabs without icons, counting only what is non-zero', async () => {
    const el = await mount('build-win');
    const tabs = Array.from(el.querySelectorAll('[role="tab"]'));
    expect(tabs).toHaveLength(5);
    expect(tabs.some((t) => t.querySelector('mat-icon'))).toBe(false);
    expect(text(tabs[0])).toBe('Overview');
    expect(text(tabs[2])).toMatch(/^Processes\s?3$/);
  });

  it('sizes the overview cards to their content', async () => {
    const el = await mount('build-win');
    expect(getComputedStyle(el.querySelector('.grid') as HTMLElement).alignItems).toBe('start');
    expect(el.querySelector('.where .kv')).not.toBeNull();
  });

  it('links the Updates tab to every runner\u2019s updates and the Disk tab to Storage', async () => {
    await mock.status();
    const f = TestBed.createComponent(RunnerDetailPage);
    f.componentRef.setInput('name', 'build-win');
    f.componentRef.setInput('tab', 'updates');
    await settle(f, 6);
    expect((f.nativeElement as HTMLElement).querySelector('.xlink a')?.getAttribute('href')).toBe('/updates');
    f.componentRef.setInput('tab', 'disk');
    await settle(f, 6);
    const link = (f.nativeElement as HTMLElement).querySelector('.xlink a');
    expect(text(link)).toBe('See Storage');
    expect(link?.getAttribute('href')).toBe('/storage');
  });

  it('says so when the runner is not running', async () => {
    const el = await mount('docker-lab');
    expect(text(el.querySelector('[aria-labelledby="use-title"]'))).toContain('Not running, so nothing is used.');
  });

  it('copes with a history that cannot load', async () => {
    vi.spyOn(mock, 'metrics').mockRejectedValue(new Error('no'));
    const el = await mount('build-win');
    expect(el.querySelectorAll('[aria-labelledby="use-title"] pc-sparkline')).toHaveLength(0);
  });
});
