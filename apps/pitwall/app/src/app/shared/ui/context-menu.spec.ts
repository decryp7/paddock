import { TestBed } from '@angular/core/testing';
import { ContextMenuService } from './context-menu';

describe('ContextMenuService', () => {
  const items = () => Array.from(document.querySelectorAll<HTMLButtonElement>('.pw-menu .pw-menu-item'));

  it('opens one menu at a time, runs an item and closes', () => {
    const svc = TestBed.inject(ContextMenuService);
    const ran: string[] = [];
    svc.open(10, 20, [{ label: 'One', run: () => ran.push('one') }]);
    svc.open(30, 40, [{ label: 'Two', run: () => ran.push('two') }]);
    expect(items().map((b) => b.textContent?.trim())).toEqual(['Two']);
    items()[0]!.click();
    expect(ran).toEqual(['two']);
    expect(items().length).toBe(0);
  });

  it('closes on Escape, on a click outside and on another right-click', () => {
    const svc = TestBed.inject(ContextMenuService);
    svc.open(1, 1, [{ label: 'A', run: () => undefined }]);
    document.querySelector('.pw-menu [cdkMenu], .pw-menu')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    document.querySelector('.cdk-overlay-backdrop')?.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(items().length).toBe(0);
    svc.open(1, 1, [{ label: 'A', run: () => undefined }]);
    const menu = new MouseEvent('contextmenu', { bubbles: true, cancelable: true });
    document.querySelector('.cdk-overlay-backdrop')!.dispatchEvent(menu);
    expect(menu.defaultPrevented).toBe(true);
    expect(items().length).toBe(0);
    svc.close();
  });
});
