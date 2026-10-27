import { TestBed } from '@angular/core/testing';
import { TOAST_MS, ToastStore } from './toast.store';

const toast = (tone: 'success' | 'danger', title: string) => ({ tone, icon: 'check', title, body: null });

describe('ToastStore', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    TestBed.resetTestingModule();
  });
  afterEach(() => vi.useRealTimers());

  it('shows toasts with increasing ids and dismisses them', () => {
    const store = TestBed.inject(ToastStore);
    const a = store.show(toast('success', 'a'));
    const b = store.show(toast('success', 'b'));
    expect(b).toBeGreaterThan(a);
    expect(store.toasts().map((t) => t.title)).toEqual(['a', 'b']);
    store.dismiss(a);
    expect(store.toasts().map((t) => t.title)).toEqual(['b']);
  });

  it('removes non-errors after a while but keeps errors', () => {
    const store = TestBed.inject(ToastStore);
    store.show(toast('success', 'ok'));
    store.show(toast('danger', 'bad'));
    vi.advanceTimersByTime(TOAST_MS + 1);
    expect(store.toasts().map((t) => t.title)).toEqual(['bad']);
  });

  it('keeps the newest four', () => {
    const store = TestBed.inject(ToastStore);
    for (let i = 0; i < 6; i++) store.show(toast('danger', `t${i}`));
    expect(store.toasts().map((t) => t.title)).toEqual(['t2', 't3', 't4', 't5']);
  });
});
