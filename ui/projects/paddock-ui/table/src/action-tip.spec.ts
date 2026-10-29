import { ChangeDetectionStrategy, Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MatTooltip } from '@angular/material/tooltip';
import { By } from '@angular/platform-browser';
import { PdActionTip } from './action-tip';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdActionTip],
  template: `<span pdActionTip><button class="a" aria-label="Edit rule">e</button><button class="b" aria-label="Delete rule">d</button><i class="gap">-</i></span>`,
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

  it('is empty again once the pointer has left, so nothing is described twice', () => {
    const { f, tip, over } = setup();
    over('.a');
    f.nativeElement.querySelector('span').dispatchEvent(new MouseEvent('mouseleave'));
    expect(tip.message).toBe('');
  });
});
