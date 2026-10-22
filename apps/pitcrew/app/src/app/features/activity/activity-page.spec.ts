import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { ActivityPage } from './activity-page';

describe('ActivityPage', () => {
  let mock: MockBackend;
  afterEach(() => mock.dispose());

  async function mount() {
    localStorage.clear();
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
    const f = TestBed.createComponent(ActivityPage);
    await settle(f, 8);
    return { f, el: f.nativeElement as HTMLElement };
  }

  it('lists the recent jobs with the server\'s result when a token is known', async () => {
    const { el } = await mount();
    expect(text(el)).toContain('Recent jobs');
    expect(text(el.querySelector('thead'))).toContain('Result');
    expect(el.querySelectorAll('tbody tr').length).toBeGreaterThan(0);
    expect(el.querySelectorAll('tbody .badge').length + el.querySelectorAll('tbody td.faint').length).toBeGreaterThan(0);
  });

  it('turns a conclusion into a badge, and no conclusion into nothing', async () => {
    const { f } = await mount();
    const page = f.componentInstance as unknown as { outcome(c: string | undefined): { label: string; tone: string } | null };
    expect(page.outcome('success')).toEqual({ label: 'Succeeded', tone: 'success' });
    expect(page.outcome('failure')?.tone).toBe('danger');
    expect(page.outcome('cancelled')?.label).toBe('Cancelled');
    expect(page.outcome('skipped')?.label).toBe('Skipped');
    expect(page.outcome('unknown')).toBeNull();
    expect(page.outcome(undefined)).toBeNull();
  });
});
