import { LiveAnnouncer } from '@angular/cdk/a11y';
import { TestBed } from '@angular/core/testing';
import { provideIcons } from './icons';
import { Notifier, Toasts } from './notifier';

function setup() {
  TestBed.configureTestingModule({ providers: [provideIcons()] });
  const announce = vi.spyOn(TestBed.inject(LiveAnnouncer), 'announce').mockResolvedValue(undefined);
  return { n: TestBed.inject(Notifier), announce };
}

describe('Notifier', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('shows a toast with a default ok tone, announces politely and expires', () => {
    const { n, announce } = setup();
    n.show('Saved');
    expect(n.toasts()).toMatchObject([{ text: 'Saved', tone: 'ok', action: null }]);
    expect(announce).toHaveBeenCalledWith('Saved', 'polite');
    vi.advanceTimersByTime(4999);
    expect(n.toasts()).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(n.toasts()).toHaveLength(0);
  });

  it('keeps a toast with ms 0 until dismissed', () => {
    const { n } = setup();
    n.show('Sticky', 'warn', null, 0);
    vi.advanceTimersByTime(60_000);
    expect(n.toasts()).toHaveLength(1);
    n.dismiss(n.toasts()[0]!.id);
    expect(n.toasts()).toHaveLength(0);
  });

  it('announces errors assertively and keeps them longer', () => {
    const { n, announce } = setup();
    n.error({ code: 'busy', message: 'Dyno is busy' });
    expect(announce).toHaveBeenCalledWith('Dyno is busy', 'assertive');
    expect(n.toasts()[0]!.tone).toBe('bad');
    vi.advanceTimersByTime(7999);
    expect(n.toasts()).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(n.toasts()).toHaveLength(0);
  });

  it('keeps only the latest four toasts with unique ids', () => {
    const { n } = setup();
    for (let i = 0; i < 6; i++) n.show('t' + i, 'ok', null, 0);
    expect(n.toasts().map((t) => t.text)).toEqual(['t2', 't3', 't4', 't5']);
    expect(new Set(n.toasts().map((t) => t.id)).size).toBe(4);
  });

  it('dismissing an unknown id changes nothing', () => {
    const { n } = setup();
    n.show('a', 'ok', null, 0);
    n.dismiss(9999);
    expect(n.toasts()).toHaveLength(1);
  });
});

describe('Toasts', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('renders toasts with tone, runs an action and dismisses', () => {
    const { n } = setup();
    const f = TestBed.createComponent(Toasts);
    const el = f.nativeElement as HTMLElement;
    const run = vi.fn();
    n.show('Done', 'ok', { label: 'Undo', run }, 0);
    n.show('Broke', 'bad', null, 0);
    f.detectChanges();
    const toasts = [...el.querySelectorAll<HTMLElement>('.toast')];
    expect(toasts.map((t) => t.dataset['tone'])).toEqual(['ok', 'bad']);
    expect(toasts[1]!.querySelector('button.btn:not(.icon)')).toBeNull();
    [...toasts[0]!.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Undo')!.click();
    f.detectChanges();
    expect(run).toHaveBeenCalledTimes(1);
    expect(el.querySelectorAll('.toast')).toHaveLength(1);
    (el.querySelector('button[aria-label="Dismiss"]') as HTMLButtonElement).click();
    f.detectChanges();
    expect(el.querySelectorAll('.toast')).toHaveLength(0);
  });
});
