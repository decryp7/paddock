import { TestBed } from '@angular/core/testing';
import { ActivatedRoute } from '@angular/router';
import { PAGE_DATA } from '@decryp7/paddock-ui/shell/pages';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { fakeChartProvider, settle } from '../../testing/render';
import { NAV } from './nav';
import { PitwallPage, navItemOf } from './page';

function mount(pageId: string | undefined) {
  TestBed.configureTestingModule({
    providers: [{ provide: BACKEND, useValue: new MockBackend() }, fakeChartProvider, { provide: ActivatedRoute, useValue: { snapshot: { data: pageId ? { [PAGE_DATA]: pageId } : {} } } }],
  });
  const fixture = TestBed.createComponent(PitwallPage);
  return { fixture, el: fixture.nativeElement as HTMLElement };
}

describe('PitwallPage', () => {
  it('finds a nav entry by page id, and nothing for another app’s page', () => {
    expect(navItemOf('pw.apps')?.label).toBe('Apps & keys');
    expect(navItemOf('dy.apps')).toBeUndefined();
    expect(navItemOf(undefined)).toBeUndefined();
  });

  it('shows the page of its route under the service’s notices and the running jobs', async () => {
    const benchmarks = NAV.find((n) => n.id === 'benchmarks')!;
    const { fixture, el } = mount('pw.benchmarks');
    await benchmarks.load();
    await settle(fixture);
    expect(el.querySelector('pw-service-notices')).not.toBeNull();
    expect(el.querySelector('pw-jobs-panel')).not.toBeNull();
    expect(el.querySelector('pw-benchmarks-page')).not.toBeNull();
  });

  it('renders nothing for a page that is not Pitwall’s', async () => {
    const { fixture, el } = mount(undefined);
    await settle(fixture);
    expect(el.querySelector('pw-dashboard')).toBeNull();
  });
});
