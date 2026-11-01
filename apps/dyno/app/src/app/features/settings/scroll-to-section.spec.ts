import { scrollToSection } from './scroll-to-section';

describe('scrollToSection', () => {
  let resize: (() => void) | null = null;
  let disconnected = false;

  beforeEach(() => {
    document.body.innerHTML = '<main><h3 id="target">Popup widgets</h3></main>';
    Element.prototype.scrollIntoView = () => undefined;
    resize = null;
    disconnected = false;
    vi.stubGlobal(
      'ResizeObserver',
      class {
        constructor(cb: () => void) {
          resize = cb;
        }
        observe(): void {}
        disconnect(): void {
          disconnected = true;
        }
      },
    );
  });
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('scrolls to the section and focuses it, and scrolls again when the content around it changes', () => {
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
    const stop = scrollToSection('target', { view: null });
    expect(scroll).toHaveBeenCalledTimes(1);
    expect(scroll.mock.contexts[0]).toBe(document.getElementById('target'));
    expect(document.activeElement).toBe(document.getElementById('target'));
    resize?.();
    expect(scroll).toHaveBeenCalledTimes(2);
    stop();
  });

  it('lets go once the person scrolls', () => {
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
    scrollToSection('target', { view: null });
    document.dispatchEvent(new Event('wheel'));
    expect(disconnected).toBe(true);
    scroll.mockClear();
    document.dispatchEvent(new Event('wheel'));
    expect(scroll).not.toHaveBeenCalled();
  });

  it('scrolls only two frames after its window came forward, and logs it (a re-targeted Settings window was left blank)', () => {
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
    const frames: FrameRequestCallback[] = [];
    let onFocus: (() => void) | null = null;
    let focused = false;
    const view = {
      document: { hasFocus: () => focused },
      addEventListener: (_: 'focus', l: () => void) => (onFocus = l),
      removeEventListener: () => (onFocus = null),
      requestAnimationFrame: (cb: FrameRequestCallback) => frames.push(cb),
      cancelAnimationFrame: () => undefined,
    };
    const frame = () => frames.splice(0).forEach((cb) => cb(0));
    const lines: string[] = [];
    const stop = scrollToSection('target', { view, log: (m) => lines.push(m) });
    frame();
    expect(scroll, 'scrolled before the window was active').not.toHaveBeenCalled();
    focused = true;
    onFocus!();
    frame();
    expect(scroll).not.toHaveBeenCalled();
    frame();
    expect(scroll).toHaveBeenCalledTimes(1);
    expect(lines).toEqual([expect.stringMatching(/^Settings: scrolled to section target after the window came forward \(scrollTop \d+\)$/)]);
    stop();
  });

  it('stopped before its window came forward, it never scrolls', () => {
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
    let onFocus: (() => void) | null = null;
    const view = {
      document: { hasFocus: () => false },
      addEventListener: (_: 'focus', l: () => void) => (onFocus = l),
      removeEventListener: () => (onFocus = null),
      requestAnimationFrame: () => 1,
      cancelAnimationFrame: () => undefined,
    };
    scrollToSection('target', { view })();
    expect(onFocus).toBeNull();
    expect(scroll).not.toHaveBeenCalled();
  });

  it('does nothing when the section is not there', () => {
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
    scrollToSection('missing', { view: null })();
    expect(scroll).not.toHaveBeenCalled();
  });
});
