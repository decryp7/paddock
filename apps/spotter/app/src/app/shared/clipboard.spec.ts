import { TestBed } from '@angular/core/testing';
import { ToastStore } from '../state/toast.store';
import { Clipboard } from './clipboard';

describe('Clipboard', () => {
  afterEach(() => vi.restoreAllMocks());

  it('copies and confirms with a toast', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await TestBed.inject(Clipboard).copy('hello', 'The path');
    expect(writeText).toHaveBeenCalledWith('hello');
    expect(TestBed.inject(ToastStore).toasts().at(-1)).toMatchObject({ tone: 'success', title: 'The path copied' });
  });

  it('explains when the clipboard is not available', async () => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: vi.fn().mockRejectedValue(new Error('no')) }, configurable: true });
    await TestBed.inject(Clipboard).copy('x', 'It');
    const t = TestBed.inject(ToastStore).toasts().at(-1)!;
    expect(t.tone).toBe('danger');
    expect(t.title).toBe('Could not copy');
    expect(t.body).toContain('keyboard');
  });
});
