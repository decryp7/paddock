import { TestBed } from '@angular/core/testing';
import type { PopoverStatus } from '../../core/generated/widgets';
import { appProviders, makeMock, settle, text } from '../../../testing/test-helpers';
import { StatusPill } from './status-pill';

async function pill(status: PopoverStatus | null) {
  TestBed.configureTestingModule({ providers: appProviders(makeMock()) });
  const f = TestBed.createComponent(StatusPill);
  f.componentRef.setInput('status', status);
  await settle(f);
  return f.nativeElement as HTMLElement;
}

describe('StatusPill', () => {
  it('says how the watches are doing in words and an icon, tinted by tone', async () => {
    const failing = await pill({ kind: 'failing', count: 2 });
    expect(text(failing.querySelector('.pill'))).toBe('2 failing');
    expect(failing.querySelector('.pill')?.getAttribute('data-tone')).toBe('danger');
    expect(failing.querySelector('.pill mat-icon')).not.toBeNull();
    TestBed.resetTestingModule();
    const good = await pill({ kind: 'allGood' });
    expect(text(good.querySelector('.pill'))).toBe('All good');
    expect(good.querySelector('.pill')?.getAttribute('data-tone')).toBe('success');
    TestBed.resetTestingModule();
    const unchecked = await pill({ kind: 'unchecked', count: 3 });
    expect(text(unchecked.querySelector('.pill'))).toBe('3 not checked yet');
  });

  it('says nothing when there is nothing to say', async () => {
    expect((await pill({ kind: 'unknown' })).querySelector('.pill')).toBeNull();
    TestBed.resetTestingModule();
    expect((await pill(null)).querySelector('.pill')).toBeNull();
  });
});
