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
  afterEach(() => vi.unstubAllGlobals());

  it('scrolls to the section and focuses it, and scrolls again when the content around it changes', () => {
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
    const stop = scrollToSection('target');
    expect(scroll).toHaveBeenCalledTimes(1);
    expect(scroll.mock.contexts[0]).toBe(document.getElementById('target'));
    expect(document.activeElement).toBe(document.getElementById('target'));
    resize?.();
    expect(scroll).toHaveBeenCalledTimes(2);
    stop();
  });

  it('lets go once the person scrolls', () => {
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
    scrollToSection('target');
    document.dispatchEvent(new Event('wheel'));
    expect(disconnected).toBe(true);
    scroll.mockClear();
    document.dispatchEvent(new Event('wheel'));
    expect(scroll).not.toHaveBeenCalled();
  });

  it('does nothing when the section is not there', () => {
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView');
    scrollToSection('missing')();
    expect(scroll).not.toHaveBeenCalled();
  });
});
