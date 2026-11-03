import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { IntelStore } from '../../state/intel.store';
import { OnlineData } from './online-data';

describe('OnlineData first-run note', () => {
  it('keeps "(download.db-ip.com)" whole, parentheses inside, on one line', async () => {
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: new MockBackend({ latencyMs: 0 }) }] });
    const store = TestBed.inject(IntelStore);
    await store.load();
    const f = TestBed.createComponent(OnlineData);
    f.detectChanges();
    const nobr = (f.nativeElement as HTMLElement).querySelector('.nobr')!;
    expect(nobr.textContent!.trim()).toBe('(download.db-ip.com)');
    expect(getComputedStyle(nobr).whiteSpace).toBe('nowrap');
  });
});
