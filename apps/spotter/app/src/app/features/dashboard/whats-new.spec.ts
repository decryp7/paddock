import { TestBed } from '@angular/core/testing';
import { WhatsNew } from './whats-new';
import type { WhatsNewItem } from '../../core/api.types';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';

const item = (over: Partial<WhatsNewItem> = {}, release: Partial<WhatsNewItem['release']> = {}): WhatsNewItem => ({
  watchId: 'w-1',
  watchName: 'Acme Installer',
  sourceKind: 'folder',
  release: { version: '4.0.0-rc.1', prerelease: true, name: 'n', location: 'l', publishedAtMs: null, firstSeenMs: 1, size: null, sha256: null, product: null, channel: '', fields: {}, artifacts: [], updatedInPlace: false, ...release },
  releasedAtMs: 1_000,
  newerThanInstalled: false,
  unseen: false,
  ...over,
});

describe('WhatsNew', () => {
  function make(items: WhatsNewItem[]) {
    TestBed.configureTestingModule({ providers: appProviders(makeMock()) });
    const f = TestBed.createComponent(WhatsNew);
    f.componentRef.setInput('items', items);
    return f;
  }

  it('flags pre-releases, omits an unknown size and offers no Mark seen for a seen release', async () => {
    const f = make([item()]);
    await settle(f);
    const t = text(f.nativeElement);
    expect(t).toContain('4.0.0-rc.1');
    expect(t).toContain('Pre-release');
    expect(t).not.toContain('New');
    expect(t).not.toMatch(/MiB|KiB/);
    expect(f.nativeElement.querySelector('button[aria-label^="Mark "]')).toBeNull();
    expect(f.nativeElement.querySelector('li.unseen')).toBeNull();
  });

  it('shows an empty state', async () => {
    const f = make([]);
    await settle(f);
    expect(text(f.nativeElement)).toContain('No releases yet');
  });
});
