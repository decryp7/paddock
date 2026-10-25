import { TestBed } from '@angular/core/testing';
import { TOAST_MS, ToastStore } from './toast.store';

describe('ToastStore', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('shows, caps and dismisses toasts', () => {
    const s = TestBed.inject(ToastStore);
    for (let i = 0; i < 6; i++) s.success(`t${i}`);
    expect(s.toasts().length).toBe(4);
    expect(s.toasts()[0]?.title).toBe('t2');
    s.dismiss(s.toasts()[0]!.id);
    expect(s.toasts().length).toBe(3);
  });

  it('removes a toast after a while, but keeps errors until dismissed', () => {
    const s = TestBed.inject(ToastStore);
    s.success('ok');
    const err = s.error('bad', 'why');
    vi.advanceTimersByTime(TOAST_MS + 1);
    expect(s.toasts().map((t) => t.id)).toEqual([err]);
    expect(s.toasts()[0]?.body).toBe('why');
  });
});
