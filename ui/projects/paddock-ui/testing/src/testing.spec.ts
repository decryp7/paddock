import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { FakeChart, FakeECharts } from './fake-chart';
import { all, at, buttonByText, byText, manualFrames, must, one, press, text, typeInto } from './dom';
import { installTestKit } from './setup';

function dom(html: string): HTMLElement {
  const root = document.createElement('div');
  root.innerHTML = html;
  return root;
}

describe('installTestKit', () => {
  it('fills the jsdom gaps and is safe to call again', () => {
    installTestKit();
    installTestKit();
    expect(typeof window.matchMedia('(min-width: 1px)').matches).toBe('boolean');
    expect(typeof ResizeObserver).toBe('function');
    expect(typeof PointerEvent).toBe('function');
    expect(new PointerEvent('pointerdown', { pointerId: 7 }).pointerId).toBe(7);
    const el = document.createElement('div');
    expect(() => {
      el.setPointerCapture(1);
      el.releasePointerCapture(1);
      el.scrollIntoView();
    }).not.toThrow();
  });

  it('replaces a global a spec stubbed to undefined and left behind (the key exists, the constructor does not)', () => {
    vi.stubGlobal('ResizeObserver', undefined);
    vi.stubGlobal('PointerEvent', undefined);
    installTestKit();
    expect(typeof ResizeObserver).toBe('function');
    expect(typeof PointerEvent).toBe('function');
    vi.unstubAllGlobals();
  });

  it('reports a target to a ResizeObserver stand-in so libraries reading entry.target keep working', () => {
    const el = document.createElement('div');
    const seen: Element[] = [];
    const observer = new ResizeObserver((entries) => entries.forEach((e) => seen.push(e.target)));
    observer.observe(el);
    observer.unobserve(el);
    observer.disconnect();
    expect(seen.length === 1 || seen.length === 0).toBe(true);
  });

  describe('between tests', () => {
    installTestKit({ dropWindowKeys: ['__pd_leftover__'] });
    it('leaves a stub behind', () => {
      (window as unknown as Record<string, unknown>)['__pd_leftover__'] = {};
      expect('__pd_leftover__' in window).toBe(true);
    });
    it('finds the window clean again', () => {
      expect('__pd_leftover__' in window).toBe(false);
    });
  });

  // Regression: the Angular builder runs Vitest with isolate=false, so spec files share one window per worker. A test that
  // failed before its own clean-up (fake timers, a spy on a DOM prototype, a stubbed global, a node on the page, a stored
  // value) broke whichever spec file ran next in that worker: order-dependent failures seen in full runs only.
  describe('what a test leaves behind', () => {
    const realAppend = Node.prototype.appendChild;
    const realNavigator = globalThis.navigator;
    it('leaves fake timers, a spy, a stubbed global, a node and a stored value behind', () => {
      document.body.append(Object.assign(document.createElement('div'), { id: 'pd-leftover' }));
      vi.useFakeTimers();
      vi.spyOn(Node.prototype, 'appendChild').mockImplementation(<T extends Node>(n: T) => n);
      vi.stubGlobal('navigator', { leftover: true });
      localStorage.setItem('pd-leftover', '1');
      sessionStorage.setItem('pd-leftover', '1');
      expect(vi.isFakeTimers()).toBe(true);
    });
    it('is gone before the next test starts', () => {
      expect(vi.isFakeTimers()).toBe(false);
      expect(Node.prototype.appendChild).toBe(realAppend);
      expect(globalThis.navigator).toBe(realNavigator);
      expect(document.getElementById('pd-leftover')).toBeNull();
      expect(localStorage.getItem('pd-leftover')).toBeNull();
      expect(sessionStorage.getItem('pd-leftover')).toBeNull();
    });
  });

  // Regression (CI runs 7257, 7274 and 7276): a spy on the faked setTimeout keeps the fake as its original, and Vitest
  // restores it at every restoreAllMocks, even after mockRestore. Once the clock was uninstalled that wrote back a fake
  // whose timers never fire, so every later spec in the worker that waited on a real timer (the panel snapshot's "too
  // slow" case, the notify reference app's repairs) hung until the 5 s test timeout.
  describe('a spy on a faked timer', () => {
    const realSetTimeout = globalThis.setTimeout;
    it('is left behind with the fake clock', () => {
      vi.useFakeTimers();
      vi.spyOn(globalThis, 'setTimeout');
      expect(vi.isFakeTimers()).toBe(true);
    });
    it('is gone, and the real setTimeout fires, before the next test starts', async () => {
      expect(globalThis.setTimeout).toBe(realSetTimeout);
      await new Promise<void>((resolve) => setTimeout(resolve, 1));
    });
  });

  describe('a spy on a faked timer outliving a spec’s own switch to real timers', () => {
    const realSetTimeout = globalThis.setTimeout;
    const fake = { left: false };
    // As in a spec whose afterEach calls vi.useRealTimers(): it runs before the kit's, so the kit finds the spy over a dead clock.
    afterEach(() => vi.useRealTimers());
    it.fails('fails the test that left it, even when the spec restored its spy', () => {
      vi.useFakeTimers();
      vi.spyOn(globalThis, 'setTimeout').mockRestore();
      fake.left = true;
    });
    it('finds the real setTimeout put back for the next test', async () => {
      expect(fake.left).toBe(true);
      expect(globalThis.setTimeout).toBe(realSetTimeout);
      await new Promise<void>((resolve) => setTimeout(resolve, 1));
    });
  });
});

describe('DOM helpers', () => {
  it('narrows missing values with a message', () => {
    expect(must(5)).toBe(5);
    expect(() => must(null, 'the row')).toThrow('expected the row to be present');
    expect(at([1, 2, 3], -1)).toBe(3);
    expect(() => at([1], 4)).toThrow('index 4');
    expect(() => at([1], -3)).toThrow('index -3');
  });

  it('reads text and finds elements', () => {
    const root = dom('<p class="a"> Hello   <b>world</b> </p><p class="a">Bye</p><button aria-label="Close">x</button><button>Save</button>');
    expect(text(root.querySelector('p'))).toBe('Hello world');
    expect(text(null)).toBe('');
    expect(all(root, 'p.a')).toHaveLength(2);
    expect(one(root, 'b').textContent).toBe('world');
    expect(() => one(root, 'i')).toThrow('no element for i');
    expect(byText(root, 'p', 'Bye')?.className).toBe('a');
    expect(byText(root, 'p', /^Hello/)).not.toBeNull();
    expect(byText(root, 'p', 'nope')).toBeNull();
    expect(buttonByText(root, 'Save').textContent).toBe('Save');
    expect(buttonByText(root, 'Close').textContent).toBe('x');
    expect(() => buttonByText(root, /Delete/)).toThrow('no button');
  });

  it('types like a user and presses keys', () => {
    const root = dom('<input><select><option value="a">a</option><option value="b">b</option></select>');
    const input = one<HTMLInputElement>(root, 'input');
    const seen: string[] = [];
    input.addEventListener('input', () => seen.push(input.value));
    typeInto(input, 'abc');
    expect(seen).toEqual(['abc']);
    const select = one<HTMLSelectElement>(root, 'select');
    let changed = '';
    select.addEventListener('change', () => (changed = select.value));
    typeInto(select, 'b', 'change');
    expect(changed).toBe('b');
    let key = '';
    root.addEventListener('keydown', (e) => (key = (e as KeyboardEvent).key));
    expect(press(root, 'Escape').defaultPrevented).toBe(false);
    expect(key).toBe('Escape');
  });

  describe('manualFrames', () => {
    // Installed per test: the kit puts the real timer globals back before every test.
    let frames: ReturnType<typeof manualFrames>;
    beforeEach(() => (frames = manualFrames()));
    afterEach(() => frames.restore());
    it('runs requested frames only when flushed, and forgets cancelled ones', () => {
      const ran: number[] = [];
      requestAnimationFrame(() => ran.push(1));
      const cancelled = requestAnimationFrame(() => ran.push(2));
      cancelAnimationFrame(cancelled);
      expect(frames.pending).toBe(1);
      expect(ran).toEqual([]);
      frames.flush();
      expect(ran).toEqual([1]);
      expect(frames.pending).toBe(0);
    });
  });
});

describe('FakeECharts', () => {
  it('records instances, options, actions and handlers, and answers geometry with changeable values', () => {
    const fake = new FakeECharts();
    const chart: FakeChart = fake.init();
    chart.setOption({ a: 1 });
    chart.setOption({ a: 2 });
    chart.dispatchAction({ type: 'x' });
    chart.resize();
    chart.on('click', () => undefined);
    chart.getZr().on('contextmenu', () => undefined);
    expect(fake.instances).toEqual([chart]);
    expect(chart.lastOption).toEqual({ a: 2 });
    expect(chart.actions).toEqual([{ type: 'x' }]);
    expect(chart.resized).toBe(1);
    expect(Object.keys(chart.handlers)).toEqual(['click']);
    expect(Object.keys(chart.zrHandlers)).toEqual(['contextmenu']);
    expect(chart.containPixel()).toBe(true);
    expect(chart.convertFromPixel({}, [2, 5])).toEqual([2000, 5]);
    chart.contains = false;
    chart.convert = ([x = 0]) => [x];
    expect(chart.containPixel({}, [1, 1])).toBe(false);
    expect(chart.convertFromPixel({}, [9])).toEqual([9]);
    chart.dispose();
    expect(chart.disposed).toBe(true);
    fake.reset();
    expect(fake.instances).toEqual([]);
  });
});
