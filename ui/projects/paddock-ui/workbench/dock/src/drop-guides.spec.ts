import { DropGuides } from './drop-guides';

describe('DropGuides', () => {
  it('follows a pointer drag from a tab: the compass over the group under it, the edge guides, the hit cell', () => {
    const root = document.createElement('div');
    root.innerHTML = '<div class="dv-tab"></div><div class="other"></div><div class="dv-groupview"><div class="dv-content-container"></div></div>';
    document.body.appendChild(root);
    const rect = (x: number, y: number, w: number, h: number) => () => ({ left: x, top: y, width: w, height: h, right: x + w, bottom: y + h, x, y, toJSON: () => ({}) }) as DOMRect;
    root.getBoundingClientRect = rect(0, 0, 800, 600);
    const content = root.querySelector<HTMLElement>('.dv-content-container')!;
    content.getBoundingClientRect = rect(100, 100, 400, 300);
    const original = document.elementFromPoint;
    document.elementFromPoint = () => content;
    const guides = new DropGuides(root);
    const layer = root.querySelector('.pd-dock-drop-guides')!;
    const pointer = (type: string, target: EventTarget, x: number, y: number, button = 0) => target.dispatchEvent(new PointerEvent(type, { bubbles: true, clientX: x, clientY: y, button }));
    // Presses elsewhere, with another button, or a press that does not move far enough: no guides.
    pointer('pointerdown', root.querySelector('.other')!, 0, 0);
    pointer('pointermove', window, 300, 300);
    pointer('pointerdown', root.querySelector('.dv-tab')!, 0, 0, 2);
    pointer('pointermove', window, 300, 300);
    pointer('pointerdown', root.querySelector('.dv-tab')!, 0, 0);
    pointer('pointermove', window, 2, 2);
    expect(layer.classList.contains('on')).toBe(false);
    // The compass centre of the 400x300 content at (100,100) is at (300,250).
    pointer('pointermove', window, 300, 250);
    expect(layer.classList.contains('on')).toBe(true);
    expect(root.querySelectorAll('.pd-dock-guide-edges .pd-dock-guide')).toHaveLength(4);
    expect(root.querySelectorAll('.pd-dock-guide-compass .pd-dock-guide')).toHaveLength(5);
    expect(root.querySelector('.pd-dock-guide.hot')!.getAttribute('data-position')).toBe('center');
    document.elementFromPoint = () => null;
    pointer('pointermove', window, 26, 300);
    expect(root.querySelectorAll('.pd-dock-guide-compass .pd-dock-guide')).toHaveLength(0);
    expect(root.querySelector('.pd-dock-guide-edges .pd-dock-guide.hot')!.getAttribute('data-position')).toBe('left');
    pointer('pointerup', window, 26, 300);
    expect(layer.classList.contains('on')).toBe(false);
    // Driven from outside (a pop-out over the window), with a preview of the drop.
    guides.showAt({ x: 26, y: 300 }, { x: 0, y: 0, width: 200, height: 600 });
    expect(layer.classList.contains('on')).toBe(true);
    const preview = root.querySelector<HTMLElement>('.pd-dock-guide-preview')!;
    expect([preview.hidden, preview.style.width]).toEqual([false, '200px']);
    guides.showAt({ x: 400, y: 30 });
    expect(preview.hidden).toBe(true);
    guides.hide();
    guides.hide();
    pointer('pointercancel', window, 0, 0);
    guides.dispose();
    expect(root.querySelector('.pd-dock-drop-guides')).toBeNull();
    document.elementFromPoint = original;
    root.remove();
  });

  it('shows no guides while a floating window is moved by its tab strip; Shift+drag (redock) shows them', () => {
    const root = document.createElement('div');
    root.innerHTML = '<div class="dv-resize-container"><div class="dv-void-container"></div></div>';
    document.body.appendChild(root);
    const guides = new DropGuides(root);
    const layer = root.querySelector('.pd-dock-drop-guides')!;
    const handle = root.querySelector('.dv-void-container')!;
    const pointer = (type: string, target: EventTarget, x: number, shiftKey = false) =>
      target.dispatchEvent(new PointerEvent(type, { bubbles: true, clientX: x, clientY: 0, button: 0, pointerType: 'mouse', shiftKey }));
    pointer('pointerdown', handle, 0);
    pointer('pointermove', window, 100);
    expect(layer.classList.contains('on')).toBe(false);
    pointer('pointerup', window, 100);
    pointer('pointerdown', handle, 0, true);
    pointer('pointermove', window, 100);
    expect(layer.classList.contains('on')).toBe(true);
    guides.dispose();
    root.remove();
  });

  it('outlines itself on request and is plain again after', () => {
    const root = document.createElement('div');
    document.body.appendChild(root);
    const guides = new DropGuides(root);
    const layer = root.querySelector('.pd-dock-drop-guides')!;
    guides.setOutline(true);
    expect(layer.getAttribute('data-redock')).toBe('outline');
    guides.setOutline(false);
    expect(layer.hasAttribute('data-redock')).toBe(false);
    root.remove();
  });
});
