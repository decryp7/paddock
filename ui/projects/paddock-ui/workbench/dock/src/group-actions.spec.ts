import { GroupActions, rovingIndex } from './group-actions';

describe('GroupActions', () => {
  it('only wires up once initialised and disposes its subscriptions', () => {
    const a = new GroupActions({ openMenu: () => undefined });
    a.dispose();
    expect(a.element.querySelectorAll('button')).toHaveLength(2);
  });
});

describe('GroupActions: one Tab stop (roving tabindex)', () => {
  it('maps arrows, Home and End to the next button, wrapping around', () => {
    expect(rovingIndex('ArrowRight', 2, 3)).toBe(0);
    expect(rovingIndex('ArrowLeft', 0, 3)).toBe(2);
    expect(rovingIndex('ArrowRight', 0, 3)).toBe(1);
    expect(rovingIndex('Home', 2, 3)).toBe(0);
    expect(rovingIndex('End', 0, 3)).toBe(2);
    expect(rovingIndex('a', 0, 3)).toBeNull();
    expect(rovingIndex('ArrowRight', 0, 0)).toBeNull();
  });

  it('keeps only one button tabbable and moves the stop with focus and arrow keys', () => {
    const a = new GroupActions({ openMenu: () => undefined });
    document.body.append(a.element);
    const buttons = [...a.element.querySelectorAll('button')];
    expect(buttons.map((b) => b.tabIndex)).toEqual([0, -1]);
    expect(a.element.getAttribute('role')).toBe('toolbar');
    buttons[0]!.focus();
    buttons[0]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowRight', bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(buttons[1]);
    expect(buttons.map((b) => b.tabIndex)).toEqual([-1, 0]);
    buttons[1]!.dispatchEvent(new KeyboardEvent('keydown', { key: 'x', bubbles: true, cancelable: true }));
    expect(document.activeElement).toBe(buttons[1]);
    a.element.remove();
  });
});
