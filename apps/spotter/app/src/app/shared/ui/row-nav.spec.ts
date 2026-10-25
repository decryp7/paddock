import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { RowItem, RowList } from './row-nav';

@Component({
  imports: [RowList, RowItem],
  template: `
    <ul spRowList>
      @for (r of rows(); track r) {
        <li spRowItem [attr.data-id]="r" (spRowActivate)="activated.push(r)">
          {{ r }} <button type="button" class="inner">x</button>
        </li>
      }
    </ul>
    <ul>
      <li spRowItem class="solo">alone</li>
    </ul>
  `,
})
class Host {
  readonly rows = signal(['a', 'b', 'c']);
  readonly activated: string[] = [];
}

function setup() {
  const f = TestBed.createComponent(Host);
  f.detectChanges();
  const items = () => Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('ul[sprowlist] > li, ul:first-child > li'));
  const key = (el: Element, k: string, init: KeyboardEventInit = {}) => {
    // CDK's key manager reads the legacy keyCode, which real keyboards always set.
    const keyCode = { ArrowDown: 40, ArrowUp: 38, Home: 36, End: 35 }[k] ?? 0;
    const e = new KeyboardEvent('keydown', { key: k, keyCode, bubbles: true, cancelable: true, ...init });
    el.dispatchEvent(e);
    f.detectChanges();
    return e;
  };
  return { f, items, key };
}

describe('row keyboard navigation', () => {
  it('puts only one row in the tab order', () => {
    const { items } = setup();
    expect(items().map((i) => i.getAttribute('tabindex'))).toEqual(['0', '-1', '-1']);
  });

  it('moves with Down and Up, and Home and End jump to the ends', () => {
    const { items, key, f } = setup();
    const [a, b, c] = items() as [HTMLElement, HTMLElement, HTMLElement];
    a.focus();
    expect(key(a, 'ArrowDown').defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(b);
    key(b, 'ArrowDown');
    expect(document.activeElement).toBe(c);
    key(c, 'ArrowUp');
    expect(document.activeElement).toBe(b);
    key(b, 'End');
    expect(document.activeElement).toBe(c);
    key(c, 'Home');
    expect(document.activeElement).toBe(a);
    f.detectChanges();
    expect(items().map((i) => i.getAttribute('tabindex'))).toEqual(['0', '-1', '-1']);
  });

  it('keeps the tab stop on the row the focus last visited', () => {
    const { items, key, f } = setup();
    const [a, b] = items() as [HTMLElement, HTMLElement];
    a.focus();
    key(a, 'ArrowDown');
    f.detectChanges();
    expect(b.getAttribute('tabindex')).toBe('0');
    expect(a.getAttribute('tabindex')).toBe('-1');
  });

  it('follows the focus when a row is focused by other means', () => {
    const { items, f } = setup();
    const c = items()[2]!;
    c.focus();
    f.detectChanges();
    expect(c.getAttribute('tabindex')).toBe('0');
  });

  it('falls back to the first row when the current one disappears', () => {
    const { items, f } = setup();
    items()[2]!.focus();
    f.componentInstance.rows.set(['a', 'b']);
    f.detectChanges();
    expect(items().map((i) => i.getAttribute('tabindex'))).toEqual(['0', '-1']);
  });

  it('leaves arrows alone inside a button of a row and with modifiers', () => {
    const { items, key } = setup();
    const a = items()[0]!;
    const inner = a.querySelector('button')!;
    expect(key(inner, 'ArrowDown').defaultPrevented).toBe(false);
    a.focus();
    expect(key(a, 'ArrowDown', { shiftKey: true }).defaultPrevented).toBe(false);
    expect(key(a, 'ArrowDown', { ctrlKey: true }).defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(a);
  });

  it('activates a row with Enter only when the row itself has the focus', () => {
    const { items, key, f } = setup();
    const b = items()[1]!;
    key(b, 'Enter');
    key(b.querySelector('button')!, 'Enter');
    expect(f.componentInstance.activated).toEqual(['b']);
  });

  it('asks for the row menu with Shift+F10 and the Menu key, at the row', () => {
    const { items, key } = setup();
    const a = items()[0]!;
    const seen: MouseEvent[] = [];
    a.addEventListener('contextmenu', (e) => seen.push(e as MouseEvent));
    key(a, 'F10', { shiftKey: true });
    key(a, 'ContextMenu');
    key(a.querySelector('button')!, 'F10', { shiftKey: true });
    expect(seen).toHaveLength(2);
    expect(seen[0]!.bubbles).toBe(true);
  });

  it('works outside a list too (always in the tab order)', () => {
    const { f } = setup();
    const solo = (f.nativeElement as HTMLElement).querySelector('.solo')!;
    expect(solo.getAttribute('tabindex')).toBe('0');
    (solo as HTMLElement).focus();
  });
});
