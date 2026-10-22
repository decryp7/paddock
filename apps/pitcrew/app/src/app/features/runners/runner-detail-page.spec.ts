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
    expect(card?.querySelectorAll('pc-sparkline')).toHaveLength(2);
    expect(text(card)).toMatch(/CPU, peak \d/);
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
