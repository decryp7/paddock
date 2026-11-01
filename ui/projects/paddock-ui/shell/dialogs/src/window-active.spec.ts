import { afterWindowActive, type ActiveWindow } from './window-active';

/** A window whose focus and frames the spec drives by hand. */
class FakeWindow implements ActiveWindow {
  focused = false;
  readonly document = { hasFocus: () => this.focused };
  private focusListeners = new Set<() => void>();
  private frames = new Map<number, FrameRequestCallback>();
  private next = 1;
  addEventListener(_: 'focus', listener: () => void): void {
    this.focusListeners.add(listener);
  }
  removeEventListener(_: 'focus', listener: () => void): void {
    this.focusListeners.delete(listener);
  }
  requestAnimationFrame(cb: FrameRequestCallback): number {
    this.frames.set(this.next, cb);
    return this.next++;
  }
  cancelAnimationFrame(handle: number): void {
    this.frames.delete(handle);
  }
  focus(): void {
    this.focused = true;
    const ls = [...this.focusListeners];
    this.focusListeners.clear();
    for (const l of ls) l();
  }
  /** Draws one frame: runs the callbacks queued before it. */
  frame(): void {
    const due = [...this.frames.values()];
    this.frames.clear();
    for (const cb of due) cb(0);
  }
  get waiting(): number {
    return this.focusListeners.size + this.frames.size;
  }
}

describe('afterWindowActive', () => {
  it('runs two frames after the window gets focus, never before (WebKit left a page scrolled earlier blank)', () => {
    const w = new FakeWindow();
    const fn = vi.fn();
    afterWindowActive(fn, w);
    w.frame();
    w.frame();
    expect(fn, 'ran while the window was not focused').not.toHaveBeenCalled();
    w.focus();
    expect(fn).not.toHaveBeenCalled();
    w.frame();
    expect(fn, 'ran one frame after the focus').not.toHaveBeenCalled();
    w.frame();
    expect(fn).toHaveBeenCalledTimes(1);
    w.focus();
    w.frame();
    w.frame();
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('in a focused window runs after two frames', () => {
    const w = new FakeWindow();
    w.focused = true;
    const fn = vi.fn();
    afterWindowActive(fn, w);
    w.frame();
    expect(fn).not.toHaveBeenCalled();
    w.frame();
    expect(fn).toHaveBeenCalledTimes(1);
  });

  it('cancelled, it never runs and leaves nothing waiting, before or after the focus', () => {
    const w = new FakeWindow();
    const fn = vi.fn();
    afterWindowActive(fn, w)();
    w.focus();
    w.frame();
    w.frame();
    const w2 = new FakeWindow();
    const cancel = afterWindowActive(fn, w2);
    w2.focus();
    w2.frame();
    cancel();
    w2.frame();
    expect(fn).not.toHaveBeenCalled();
    expect(w.waiting + w2.waiting).toBe(0);
  });

  it('without a window (server rendering) runs at once', () => {
    const fn = vi.fn();
    afterWindowActive(fn, null);
    expect(fn).toHaveBeenCalledTimes(1);
  });
});
