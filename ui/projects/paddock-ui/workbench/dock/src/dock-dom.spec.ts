import { keepGroupsUnscrolled, labelTabOverflow, overflowLabel } from './dock-dom';

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

