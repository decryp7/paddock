import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { AppMenu } from './app-menu';
import { KEY_PLATFORM } from './keymap';
import type { MenuNode, TopMenu } from './menu-model';
import { PdContextMenu, PdMenuBar } from './menu-view';

const MENUS: TopMenu[] = [
  {
    id: 'file',
    label: 'File',
    items: [
      { kind: 'item', id: 'sd.open', label: 'Open…', keybinding: 'Mod+O', enabled: true },
      { kind: 'separator' },
      { kind: 'item', id: 'sd.close', label: 'Close', enabled: false },
    ],
  },
  {
    id: 'view',
    label: 'View',
    items: [
      { kind: 'item', id: 'sd.sidebar', label: 'Sidebar', checked: true, enabled: true },
      { kind: 'item', id: 'sd.status', label: 'Status bar', checked: false, enabled: true },
      { kind: 'submenu', id: 'sd.more', label: 'More', items: [{ kind: 'item', id: 'sd.compact', label: 'Compact', checked: false, enabled: true }] },
      { kind: 'item', id: 'sd.other', label: 'Other views', enabled: true },
    ],
  },
];
const CONTEXT: MenuNode[] = [{ kind: 'item', id: 'sd.copy', label: 'Copy', keybinding: 'Mod+C', enabled: true }];

const KEY_CODES: Record<string, number> = { ArrowDown: 40, ArrowUp: 38, ArrowLeft: 37, ArrowRight: 39, Escape: 27, Enter: 13 };
function press(target: Element, key: string): void {
  target.dispatchEvent(new KeyboardEvent('keydown', { key, keyCode: KEY_CODES[key] ?? key.toUpperCase().charCodeAt(0), bubbles: true } as KeyboardEventInit));
}

@Component({
  selector: 'sd-host',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [PdContextMenu],
  template: `<pd-context-menu menu="sd.ctx" label="Row actions"><p class="row">Row</p></pd-context-menu><pd-context-menu menu="sd.none"><p class="empty">Empty</p></pd-context-menu>`,
})
class Host {}

let ran: string[];
const mode = { value: 'in-app' as 'in-app' | 'native' };

function setup<T>(component: new () => T): ComponentFixture<T> {
  ran = [];
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: KEY_PLATFORM, useValue: 'other' },
      {
        provide: AppMenu,
        useValue: {
          menus: signal(MENUS),
          items: (id: string) => (id === 'sd.ctx' ? CONTEXT : []),
          run: (id: string) => (ran.push(id), Promise.resolve(true)),
          get mode() {
            return mode.value;
          },
        },
      },
    ],
  });
  const fixture = TestBed.createComponent(component);
  document.body.appendChild(fixture.nativeElement);
  return fixture;
}

const item = (label: string) =>
  [...document.querySelectorAll<HTMLElement>('[role=menu] [role=menuitem], [role=menu] [role=menuitemcheckbox]')].find((e) => e.querySelector('.pd-menu-label')?.textContent?.trim() === label);
const openMenus = () => [...document.querySelectorAll<HTMLElement>('[role=menu]')].map((m) => m.getAttribute('aria-label'));

describe('PdMenuBar', () => {
  let fixture: ComponentFixture<PdMenuBar>;
  beforeEach(async () => {
    mode.value = 'in-app';
    fixture = setup(PdMenuBar);
    await fixture.whenStable();
  });
  afterEach(() => {
    fixture.destroy();
    document.body.replaceChildren();
  });

  const top = (label: string) => [...(fixture.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('[role=menubar] > [role=menuitem]')].find((e) => e.textContent?.trim() === label)!;

  it('is an ARIA menubar of menu triggers', () => {
    const bar = (fixture.nativeElement as HTMLElement).querySelector('[role=menubar]')!;
    expect(bar.getAttribute('aria-label')).toBe('Application menu');
    expect([...bar.querySelectorAll('[role=menuitem]')].map((e) => e.textContent?.trim())).toEqual(['File', 'View']);
  });

  it('renders items, check marks, hints, separators, disabled items and submenus with their roles', async () => {
    top('File').click();
    await fixture.whenStable();
    expect(openMenus()).toEqual(['File']);
    expect(item('Open…')?.querySelector('.pd-menu-hint')?.textContent).toBe('Ctrl+O');
    expect(item('Open…')?.getAttribute('aria-keyshortcuts')).toBe('Control+O');
    expect(document.querySelector('[role=menu] [role=separator]')).not.toBeNull();
    expect(item('Close')?.getAttribute('aria-disabled')).toBe('true');
    item('Close')!.click();
    expect(ran).toEqual([]);
    item('Open…')!.click();
    await fixture.whenStable();
    expect(ran).toEqual(['sd.open']);
    expect(openMenus()).toEqual([]);
    top('View').click();
    await fixture.whenStable();
    expect(item('Sidebar')?.getAttribute('role')).toBe('menuitemcheckbox');
    expect(item('Sidebar')?.getAttribute('aria-checked')).toBe('true');
    expect(item('Status bar')?.getAttribute('aria-checked')).toBe('false');
    item('More')!.click();
    await fixture.whenStable();
    expect(openMenus()).toEqual(['View', 'More']);
    item('Compact')!.click();
    expect(ran).toEqual(['sd.open', 'sd.compact']);
  });

  it('is keyboard operable: arrows open and move, right/left open and close submenus, Escape closes one level', async () => {
    const view = top('View');
    view.focus();
    press(view, 'ArrowDown');
    await fixture.whenStable();
    await vi.waitFor(() => expect(document.activeElement).toBe(item('Sidebar')));
    press(document.activeElement!, 'ArrowDown');
    press(document.activeElement!, 'ArrowDown');
    expect(document.activeElement).toBe(item('More'));
    press(document.activeElement!, 'ArrowRight');
    await fixture.whenStable();
    expect(openMenus()).toEqual(['View', 'More']);
    await vi.waitFor(() => expect(document.activeElement).toBe(item('Compact')));
    press(document.activeElement!, 'ArrowLeft');
    await fixture.whenStable();
    expect(openMenus()).toEqual(['View']);
    press(document.activeElement!, 'Escape');
    await fixture.whenStable();
    expect(openMenus()).toEqual([]);
    expect(document.activeElement).toBe(view);
  });

  it('jumps to an item by typing its first letters', async () => {
    const view = top('View');
    view.focus();
    press(view, 'ArrowDown');
    await fixture.whenStable();
    await vi.waitFor(() => expect(document.activeElement).toBe(item('Sidebar')));
    press(document.activeElement!, 'o');
    await vi.waitFor(() => expect(document.activeElement).toBe(item('Other views')));
  });
});

describe('PdMenuBar in native mode', () => {
  it('renders nothing: the menus are in the native menu bar', async () => {
    mode.value = 'native';
    const fixture = setup(PdMenuBar);
    await fixture.whenStable();
    expect((fixture.nativeElement as HTMLElement).querySelector('[role=menubar]')).toBeNull();
    fixture.destroy();
    mode.value = 'in-app';
  });
});

describe('PdContextMenu', () => {
  it('opens the contributed items on right-click and runs them; an empty one opens nothing', async () => {
    const fixture = setup(Host);
    await fixture.whenStable();
    const row = (fixture.nativeElement as HTMLElement).querySelector('.row')!;
    row.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 10, clientY: 10 }));
    await fixture.whenStable();
    expect(openMenus()).toEqual(['Row actions']);
    item('Copy')!.click();
    expect(ran).toEqual(['sd.copy']);
    const empty = (fixture.nativeElement as HTMLElement).querySelector('.empty')!;
    empty.dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }));
    await fixture.whenStable();
    expect(openMenus()).toEqual([]);
    fixture.destroy();
    document.body.replaceChildren();
  });
});
