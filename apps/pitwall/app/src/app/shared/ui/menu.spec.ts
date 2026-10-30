import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { registerIcons } from '../icons';
import { ItemsMenu, MoreButton, RowMenu, type MenuItem } from './menu';

@Component({
  imports: [ItemsMenu, MoreButton, RowMenu],
  template: `
    <table>
      <tbody>
        @for (r of rows; track r) {
          <tr tabindex="0" [attr.id]="'row-' + r" pwRowMenu [matContextMenuTriggerFor]="menu.menu()" [matContextMenuTriggerData]="{ items: items(r) }">
            <td>{{ r }}</td>
            <td><pw-more-button [menu]="menu.menu()" [items]="items(r)" [label]="'More actions for ' + r" /></td>
          </tr>
        }
      </tbody>
    </table>
    <pw-items-menu #menu label="Row actions" />
  `,
})
class Host {
  readonly rows = ['alpha', 'beta'];
  readonly ran = signal<string[]>([]);
  items(r: string): MenuItem[] {
    return [
      { label: `Open ${r}`, run: () => this.ran.update((l) => [...l, `open ${r}`]) },
      { label: 'Disabled', run: () => this.ran.update((l) => [...l, 'never']), disabled: true },
      { label: 'Delete…', run: () => this.ran.update((l) => [...l, `delete ${r}`]), danger: true, divider: true },
    ];
  }
}

async function setup() {
  TestBed.configureTestingModule({ imports: [Host] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const fixture = TestBed.createComponent(Host);
  document.body.appendChild(fixture.nativeElement);
  fixture.detectChanges();
  await fixture.whenStable();
  return { fixture, el: fixture.nativeElement as HTMLElement };
}

const panel = () => document.querySelector<HTMLElement>('.mat-mdc-menu-panel');
const items = () => Array.from(panel()?.querySelectorAll<HTMLButtonElement>('[mat-menu-item]') ?? []);

describe('row menus on Material (rule 47)', () => {
  afterEach(() => document.querySelector('.cdk-overlay-container')?.replaceChildren());

  it('the "..." button is an icon button with the row in its name and opens the row\'s items', async () => {
    const { fixture, el } = await setup();
    const more = el.querySelector<HTMLButtonElement>('button[aria-label="More actions for beta"]')!;
    expect(more.classList).toContain('mat-mdc-icon-button');
    expect(more.getAttribute('aria-haspopup')).toBe('menu');
    more.click();
    fixture.detectChanges();
    await fixture.whenStable();
    expect(panel()!.getAttribute('aria-label')).toBe('Row actions');
    expect(items().map((b) => b.textContent!.trim())).toEqual(['Open beta', 'Disabled', 'Delete…']);
    expect(items()[1]!.disabled).toBe(true);
    expect(items()[2]!.classList).toContain('danger');
    expect(panel()!.querySelectorAll('hr.pw-menu-divider').length).toBe(1);
    items()[2]!.click();
    expect(fixture.componentInstance.ran()).toEqual(['delete beta']);
    el.remove();
  });

  it('a right-click on a row opens the same list for that row', async () => {
    const { fixture, el } = await setup();
    el.querySelector('#row-alpha td')!.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    fixture.detectChanges();
    await fixture.whenStable();
    expect(items().map((b) => b.textContent!.trim())).toEqual(['Open alpha', 'Disabled', 'Delete…']);
    items()[0]!.click();
    expect(fixture.componentInstance.ran()).toEqual(['open alpha']);
    el.remove();
  });

  it('Shift+F10 and the Menu key on a focused row open its menu; the same keys on a control inside do not', async () => {
    const { fixture, el } = await setup();
    const row = el.querySelector<HTMLElement>('#row-beta')!;
    row.focus();
    row.dispatchEvent(new KeyboardEvent('keydown', { key: 'F10', shiftKey: true, bubbles: true, cancelable: true }));
    fixture.detectChanges();
    await fixture.whenStable();
    expect(items()[0]!.textContent!.trim()).toBe('Open beta');
    document.querySelector('.cdk-overlay-container')?.replaceChildren();
    const inner = row.querySelector<HTMLButtonElement>('button')!;
    const e = new KeyboardEvent('keydown', { key: 'ContextMenu', bubbles: true, cancelable: true });
    inner.dispatchEvent(e);
    expect(e.defaultPrevented).toBe(false);
    el.remove();
  });

  it('a menu given no list shows no items', async () => {
    TestBed.configureTestingModule({});
    const fixture = TestBed.createComponent(ItemsMenu);
    fixture.detectChanges();
    expect((fixture.componentInstance as unknown as { asItems: (x: unknown) => unknown[] }).asItems(undefined)).toEqual([]);
  });
});
