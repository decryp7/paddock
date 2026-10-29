import { ChangeDetectionStrategy, Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MatTooltip } from '@angular/material/tooltip';
import { By } from '@angular/platform-browser';
import { vi } from 'vitest';
import { PdActionTip } from './action-tip';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdActionTip],
  template: `<span pdActionTip><button class="a" aria-label="Edit rule">e</button><button class="b" aria-label="Delete rule">d</button><button class="off" disabled aria-label="Remove rule">r</button><i class="gap">-</i></span><span class="other">x</span>`,
})
class Host {}

describe('pdActionTip', () => {
  function setup() {
    const f = TestBed.createComponent(Host);
    f.detectChanges();
    const tip = f.debugElement.query(By.directive(PdActionTip)).injector.get(MatTooltip);
    const over = (sel: string) => f.nativeElement.querySelector(sel).dispatchEvent(new MouseEvent('mousemove', { bubbles: true }));
    return { f, tip, over };
  }

  it('sits left of the cluster', () => {
    expect(setup().tip.position).toBe('left');
  });

  it('shows the name of the button the pointer is over, and keeps it over the gap between buttons', () => {
    const { tip, over } = setup();
    over('.a');
    expect(tip.message).toBe('Edit rule');
    over('.b');
    expect(tip.message).toBe('Delete rule');
    over('.gap');
    expect(tip.message).toBe('Delete rule');
  });

  it('shows the name of a disabled button, which gets no pointer events of its own, from the pointer position', () => {
    const { f, tip } = setup();
    const off = f.nativeElement.querySelector('.off') as HTMLElement;
    const box = { left: 10, right: 40, top: 10, bottom: 40, width: 30, height: 30, x: 10, y: 10, toJSON: () => ({}) } as DOMRect;
    off.getBoundingClientRect = () => box;
    const cluster = f.nativeElement.querySelector('span') as HTMLElement;
    cluster.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 20, clientY: 20 }));
    expect(tip.message).toBe('Remove rule');
    // Over the cluster but not over a button: the name stays.
    cluster.dispatchEvent(new MouseEvent('mousemove', { bubbles: true, clientX: 900, clientY: 900 }));
    expect(tip.message).toBe('Remove rule');
  });

  it('shows the focused button\'s name for the keyboard (focus-visible) and hides it when the focus leaves', () => {
    const { f, tip } = setup();
    const show = vi.spyOn(tip, 'show');
    const hide = vi.spyOn(tip, 'hide');
    const a = f.nativeElement.querySelector('.a') as HTMLButtonElement;
    a.focus();
    a.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    expect(tip.message).toBe('Edit rule');
    a.dispatchEvent(new FocusEvent('focusout', { bubbles: true }));
    expect(hide).toHaveBeenCalled();
    // A pointer focus (not focus-visible) sets the name but does not open the tooltip by itself.
    show.mockClear();
    const b = f.nativeElement.querySelector('.b') as HTMLButtonElement;
    vi.spyOn(b, 'matches').mockImplementation((sel: string) => sel !== ':focus-visible');
    b.dispatchEvent(new FocusEvent('focusin', { bubbles: true }));
    expect(tip.message).toBe('Delete rule');
    expect(show).not.toHaveBeenCalled();
  });

  it('keeps the name while the pointer leaves a cluster that still has the focus', () => {
    const { f, tip, over } = setup();
    const a = f.nativeElement.querySelector('.a') as HTMLButtonElement;
    a.focus();
    over('.a');
    f.nativeElement.querySelector('span').dispatchEvent(new MouseEvent('mouseleave'));
    expect(tip.message).toBe('Edit rule');
  });

  it('is empty again once the pointer has left, so nothing is described twice', () => {
    const { f, tip, over } = setup();
    over('.a');
    f.nativeElement.querySelector('span').dispatchEvent(new MouseEvent('mouseleave'));
    expect(tip.message).toBe('');
  });
});
