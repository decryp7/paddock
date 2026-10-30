import { ChangeDetectionStrategy, Component } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { By } from '@angular/platform-browser';
import { describe, expect, it } from 'vitest';
import { isContextMenuKey, PdContextMenuKeys } from './context-menu-keys';

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdContextMenuKeys],
  template: `<div class="row" tabindex="0" pdContextMenuKeys (contextmenu)="seen.push($event)"><button class="inner">x</button></div>`,
})
class Host {
  seen: MouseEvent[] = [];
}

function setup() {
  const f = TestBed.createComponent(Host);
  f.detectChanges();
  return { f, row: f.debugElement.query(By.css('.row')).nativeElement as HTMLElement, inner: f.debugElement.query(By.css('.inner')).nativeElement as HTMLElement };
}
const key = (el: HTMLElement, init: KeyboardEventInit) => {
  const e = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
  el.dispatchEvent(e);
  return e;
};

describe('pdContextMenuKeys', () => {
  it('knows the keyboard\'s right-click', () => {
    expect(isContextMenuKey(new KeyboardEvent('keydown', { key: 'ContextMenu' }))).toBe(true);
    expect(isContextMenuKey(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true }))).toBe(true);
    expect(isContextMenuKey(new KeyboardEvent('keydown', { key: 'F10' }))).toBe(false);
  });

  it('turns Shift+F10 and the Menu key on the row into a contextmenu event', () => {
    const { f, row } = setup();
    expect(key(row, { key: 'F10', shiftKey: true }).defaultPrevented).toBe(true);
    key(row, { key: 'ContextMenu' });
    expect(f.componentInstance.seen).toHaveLength(2);
  });

  it('leaves the same keys inside a button of the row alone, and other keys', () => {
    const { f, row, inner } = setup();
    key(inner, { key: 'F10', shiftKey: true });
    key(row, { key: 'Enter' });
    expect(f.componentInstance.seen).toHaveLength(0);
  });
});
