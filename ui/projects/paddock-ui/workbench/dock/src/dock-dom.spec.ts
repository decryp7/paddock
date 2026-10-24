import { PD_ICON_PATHS } from '@decryp7/paddock-ui/icons';
import { DOCK_STYLES } from './dock-styles';
import { keepActiveTabVisible, keepGroupsUnscrolled, labelTabOverflow, overflowLabel, overflowTooltip } from './dock-dom';

describe('keepGroupsUnscrolled', () => {
  it('puts a group scrolled sideways back, leaves vertical scrolling and other elements alone, and stops when disposed', () => {
    const root = document.createElement('div');
    const group = document.createElement('div');
    group.className = 'dv-groupview';
    const other = document.createElement('div');
    root.append(group, other);
    let left = 35;
    let otherLeft = 10;
    Object.defineProperty(group, 'scrollLeft', { get: () => left, set: (v: number) => (left = v) });
    Object.defineProperty(other, 'scrollLeft', { get: () => otherLeft, set: (v: number) => (otherLeft = v) });
    const watch = keepGroupsUnscrolled(root);
    group.dispatchEvent(new Event('scroll'));
    other.dispatchEvent(new Event('scroll'));
    expect(left).toBe(0);
    expect(otherLeft).toBe(10);
    group.dispatchEvent(new Event('scroll'));
    expect(left).toBe(0);
    watch.dispose();
    left = 20;
    group.dispatchEvent(new Event('scroll'));
    expect(left).toBe(20);
  });
});

describe('overflow chip tooltip', () => {
  const rect = (left: number, right: number) => () => ({ left, right }) as DOMRect;
  it('names the hidden tabs and shows the same down chevron as the group menu', () => {
    const root = document.createElement('div');
    root.innerHTML =
      '<div class="dv-tabs-and-actions-container"><div class="dv-tabs-container">' +
      '<div class="dv-tab"><span class="pd-dock-tab-label">Files</span></div>' +
      '<div class="dv-tab"><span class="pd-dock-tab-label">Jobs</span></div>' +
      '<div class="dv-tab"><span class="pd-dock-tab-label">  </span></div></div>' +
      '<div class="dv-tabs-overflow-dropdown-root"><div class="dv-tabs-overflow-dropdown-default"><svg><path d="M0 0"/></svg><span>1</span></div></div></div>';
    document.body.appendChild(root);
    const [files, jobs, blank] = Array.from(root.querySelectorAll<HTMLElement>('.dv-tab'));
    root.querySelector<HTMLElement>('.dv-tabs-container')!.getBoundingClientRect = rect(0, 100);
    files!.getBoundingClientRect = rect(0, 60);
    jobs!.getBoundingClientRect = rect(60, 140);
    blank!.getBoundingClientRect = rect(-20, 10);
    const watch = labelTabOverflow(root);
    const chip = root.querySelector<HTMLElement>('.dv-tabs-overflow-dropdown-root')!;
    expect(chip.title).toBe('1 more tab: Jobs');
    expect(chip.getAttribute('aria-label')).toBe('1 more tab');
    expect(chip.querySelector('svg path')!.getAttribute('d')).toBe(PD_ICON_PATHS['chevron-down']);
    // The hidden set follows the width: refreshed when the pointer arrives.
    jobs!.getBoundingClientRect = rect(60, 90);
    chip.dispatchEvent(new MouseEvent('mouseover', { bubbles: true }));
    expect(chip.title).toBe('1 more tab');
    watch.dispose();
    root.remove();
  });

  it('falls back to the count alone without a strip', () => {
    expect(overflowTooltip(2, [])).toBe('2 more tabs');
    expect(overflowTooltip(2, ['A', 'B'])).toBe('2 more tabs: A, B');
    const root = document.createElement('div');
    root.innerHTML = '<div class="dv-tabs-overflow-dropdown-root"><div><span>3</span></div></div>';
    const watch = labelTabOverflow(root);
    expect(root.querySelector<HTMLElement>('.dv-tabs-overflow-dropdown-root')!.title).toBe('3 more tabs');
    watch.dispose();
  });
});

describe('labelTabOverflow', () => {
  it('makes the overflow chip a named, focusable button that opens at the chip with Enter or Space', async () => {
    vi.useFakeTimers();
    const root = document.createElement('div');
    document.body.appendChild(root);
    const watch = labelTabOverflow(root);
    root.innerHTML = '<div class="dv-tabs-overflow-dropdown-root"><div class="dv-tabs-overflow-dropdown-default"><svg></svg><span>2</span></div></div>';
    await Promise.resolve();
    const chip = root.querySelector<HTMLElement>('.dv-tabs-overflow-dropdown-root')!;
    expect([chip.getAttribute('role'), chip.tabIndex, chip.getAttribute('aria-label'), chip.title]).toEqual(['button', 0, '2 more tabs', '2 more tabs']);
    expect(chip.querySelector('span')!.getAttribute('aria-hidden')).toBe('true');
    // The count follows dockview's updates.
    chip.querySelector('span')!.textContent = '1';
    await Promise.resolve();
    expect(chip.getAttribute('aria-label')).toBe('1 more tab');
    const clicks: string[] = [];
    chip.addEventListener('click', (e) => clicks.push(`${e.clientX},${e.clientY}`));
    chip.getBoundingClientRect = () => ({ left: 40, bottom: 30 }) as DOMRect;
    for (const key of ['Enter', ' ', 'a']) chip.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true }));
    root.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(clicks).toEqual([]);
    vi.runAllTimers();
    expect(clicks).toEqual(['40,30', '40,30']);
    watch.dispose();
    chip.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    vi.runAllTimers();
    expect(clicks).toHaveLength(2);
    root.remove();
    vi.useRealTimers();
  });

  it('names the chip for one and several hidden tabs', () => {
    expect(overflowLabel(1)).toBe('1 more tab');
    expect(overflowLabel(5)).toBe('5 more tabs');
  });
});


describe('the overflow chip chevron', () => {
  it('points down: dockview turns its own chevron by 90 degrees, ours must not be turned again', () => {
    const root = document.createElement('div');
    root.innerHTML = '<div class="dv-tabs-overflow-dropdown-root"><div class="dv-tabs-overflow-dropdown-default"><svg></svg><span>3</span></div></div>';
    const watch = labelTabOverflow(root);
    const svg = root.querySelector('.dv-tabs-overflow-dropdown-default svg')!;
    expect(svg.classList.contains('pd-dock-overflow-chevron')).toBe(true);
    expect(svg.querySelector('path')!.getAttribute('d')).toBe(PD_ICON_PATHS['chevron-down']);
    // The count stays next to it.
    expect(root.querySelector('.dv-tabs-overflow-dropdown-default span')!.textContent).toBe('3');
    expect(DOCK_STYLES).toMatch(/\.dv-tabs-overflow-dropdown-default > svg\.pd-dock-overflow-chevron \{ transform: none; \}/);
    watch.dispose();
  });
});

describe('keepActiveTabVisible', () => {
  const rect = (left: number, right: number) => () => ({ left, right }) as DOMRect;
  function strip(box: [number, number], active: [number, number] | null) {
    const root = document.createElement('div');
    root.innerHTML = '<div class="dv-tabs-container"><div class="dv-tab"></div><div class="dv-tab dv-active-tab"></div></div>';
    document.body.appendChild(root);
    const el = root.querySelector<HTMLElement>('.dv-tabs-container')!;
    el.getBoundingClientRect = rect(box[0], box[1]);
    const tab = root.querySelector<HTMLElement>('.dv-active-tab')!;
    let left = 0;
    // The tab moves with the strip's scroll, as in a browser.
    const place = { at: active ?? [0, 0] };
    const tabRect = () => rect(place.at[0] - left, place.at[1] - left)();
    tab.getBoundingClientRect = tabRect;
    if (!active) tab.classList.remove('dv-active-tab');
    Object.defineProperty(el, 'scrollLeft', { get: () => left, set: (v: number) => (left = v) });
    return { root, el, tab, left: () => left, moveTo: (l: number, r: number) => (place.at = [l, r]) };
  }

  it('scrolls the strip so an active tab beyond its right edge shows, and one beyond its left edge shows', () => {
    const right = strip([0, 100], [90, 180]);
    const watch = keepActiveTabVisible(right.root);
    expect(right.left()).toBe(80);
    watch.dispose();
    const left = strip([100, 200], [60, 140]);
    const watch2 = keepActiveTabVisible(left.root);
    expect(left.left()).toBe(-40);
    watch2.dispose();
    right.root.remove();
    left.root.remove();
  });

  it('never moves a tab whose left edge would leave the strip when the tab is wider than the strip', () => {
    const wide = strip([0, 100], [10, 400]);
    const watch = keepActiveTabVisible(wide.root);
    expect(wide.left()).toBe(10);
    watch.dispose();
    wide.root.remove();
  });

  it('leaves a strip alone when its active tab is entirely inside, or it has none', () => {
    const inside = strip([0, 100], [10, 90]);
    const none = strip([0, 100], null);
    const a = keepActiveTabVisible(inside.root);
    const b = keepActiveTabVisible(none.root);
    expect([inside.left(), none.left()]).toEqual([0, 0]);
    a.dispose();
    b.dispose();
    inside.root.remove();
    none.root.remove();
  });

  it('follows the active tab when it changes and when asked to refresh', async () => {
    const m = strip([0, 100], [10, 90]);
    const watch = keepActiveTabVisible(m.root);
    m.moveTo(120, 190);
    m.tab.classList.add('another');
    await Promise.resolve();
    expect(m.left()).toBe(90);
    m.moveTo(340, 400);
    watch.refresh();
    expect(m.left()).toBe(300);
    watch.dispose();
    m.moveTo(900, 990);
    m.tab.classList.add('after-dispose');
    await Promise.resolve();
    expect(m.left()).toBe(300);
    m.root.remove();
  });
});
