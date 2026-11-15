import { signal, WritableSignal } from '@angular/core';
import { ComponentFixture, TestBed } from '@angular/core/testing';
import { settle } from '../../../testing/helpers';
import { AppMenu } from './app-menu.service';
import { MenuBar } from './menu-bar';
import { TopMenu } from './menu-model';

const MENUS: TopMenu[] = [
  {
    id: 'file',
    label: 'File',
    items: [
      { kind: 'item', id: 'file.openFiles', label: 'Open files…', keybinding: 'Mod+O' },
      { kind: 'separator' },
      { kind: 'item', id: 'file.close', label: 'Close workspace' },
    ],
  },
  {
    id: 'view',
    label: 'View',
    items: [
      { kind: 'item', id: 'view:timeline', label: 'Timeline', checked: true },
      { kind: 'item', id: 'view:events', label: 'Events', checked: false },
      {
        kind: 'submenu',
        id: 'view.analysis',
        label: 'Analysis',
        items: [
          { kind: 'item', id: 'view:anomalies', label: 'Anomalies', checked: false },
          { kind: 'item', id: 'view:patterns', label: 'Patterns', checked: false },
        ],
      },
      { kind: 'group', label: 'Windows registry', items: [{ kind: 'item', id: 'view:registry', label: 'Registry', checked: false }] },
      { kind: 'item', id: 'menu.otherWindows', label: 'Other windows…' },
    ],
  },
];

const KEY_CODES: Record<string, number> = { ArrowDown: 40, ArrowUp: 38, ArrowLeft: 37, ArrowRight: 39, Escape: 27, Enter: 13 };

function press(target: Element, key: string): void {
  const keyCode = KEY_CODES[key] ?? key.toUpperCase().charCodeAt(0);
  target.dispatchEvent(new KeyboardEvent('keydown', { key, keyCode, bubbles: true } as KeyboardEventInit));
}

describe('MenuBar', () => {
  let ran: string[];
  let fixture: ComponentFixture<MenuBar>;
  let menuModel: WritableSignal<TopMenu[]>;

  beforeEach(async () => {
    ran = [];
    menuModel = signal(MENUS);
    TestBed.configureTestingModule({ providers: [{ provide: AppMenu, useValue: { menus: menuModel, run: (id: string) => ran.push(id) } }] });
    fixture = TestBed.createComponent(MenuBar);
    document.body.appendChild(fixture.nativeElement);
    await settle(fixture);
  });

  afterEach(() => fixture.nativeElement.remove());

  const top = (label: string) => [...(fixture.nativeElement as HTMLElement).querySelectorAll<HTMLElement>('[role=menubar] > [role=menuitem]')].find((e) => e.textContent?.trim() === label)!;
  const item = (label: string) =>
    [...document.querySelectorAll<HTMLElement>('[role=menu] [role=menuitem], [role=menu] [role=menuitemcheckbox]')].find((e) => e.querySelector('.label')?.textContent?.trim() === label);
  const menus = () => [...document.querySelectorAll<HTMLElement>('[role=menu]')].map((m) => m.getAttribute('aria-label'));

  it('is an ARIA menubar of menu triggers', () => {
    const bar = fixture.nativeElement.querySelector('[role=menubar]');
    expect(bar.getAttribute('aria-label')).toBe('Application menu');
    expect(top('File').getAttribute('aria-haspopup')).toBe('menu');
    expect(top('File').getAttribute('aria-expanded')).toBe('false');
  });

  it('renders items, check marks, shortcut hints, separators, groups and submenus with their roles', async () => {
    top('View').click();
    await settle(fixture);
    expect(top('View').getAttribute('aria-expanded')).toBe('true');
    expect(menus()).toEqual(['View']);
    expect(item('Timeline')!.getAttribute('role')).toBe('menuitemcheckbox');
    expect(item('Timeline')!.getAttribute('aria-checked')).toBe('true');
    expect(item('Events')!.getAttribute('aria-checked')).toBe('false');
    expect(item('Analysis')!.getAttribute('aria-haspopup')).toBe('menu');
    const group = document.querySelector('[role=group][aria-label="Windows registry"]')!;
    expect(group.querySelector('.group-label')!.getAttribute('aria-hidden')).toBe('true');
    expect(item('Registry')!.closest('[role=group]')).toBe(group);
    top('File').click();
    await settle(fixture);
    expect(menus()).toEqual(['File']);
    expect(document.querySelectorAll('[role=separator]').length).toBe(1);
    const open = item('Open files…')!;
    expect(open.querySelector('.hint')!.textContent?.trim()).toMatch(/^(Ctrl\+O|⌘O)$/);
    expect(open.querySelector('.hint')!.getAttribute('aria-hidden')).toBe('true');
    expect(open.getAttribute('aria-keyshortcuts')).toMatch(/^(Control|Meta)\+O$/);
  });

  it('runs the chosen item and closes', async () => {
    top('View').click();
    await settle(fixture);
    item('Events')!.click();
    await settle(fixture);
    expect(ran).toEqual(['view:events']);
    expect(menus()).toEqual([]);
  });

  // A lazy command set that loads while a menu is open hides its unavailable items (Export report…), and the items below
  // move up. A button must keep running the item it shows: one rendered for "Close workspace" never runs "Close window"
  // (ux-demo-tour.e2e.ts flaked in debrief.yml run 8116 when Playwright's click landed on the reused button).
  it('keeps each open item bound to its own command when an item above it goes away', async () => {
    menuModel.set([
      {
        id: 'file',
        label: 'File',
        items: [
          { kind: 'item', id: 'report.export', label: 'Export report…' },
          { kind: 'item', id: 'file.close', label: 'Close workspace' },
          { kind: 'item', id: 'window.close', label: 'Close window' },
        ],
      },
    ]);
    await settle(fixture);
    top('File').click();
    await settle(fixture);
    const close = item('Close workspace')!;
    menuModel.set([
      {
        id: 'file',
        label: 'File',
        items: [
          { kind: 'item', id: 'file.close', label: 'Close workspace' },
          { kind: 'item', id: 'window.close', label: 'Close window' },
        ],
      },
    ]);
    await settle(fixture);
    expect(item('Export report…')).toBeUndefined();
    expect(close.querySelector('.label')!.textContent?.trim()).toBe('Close workspace');
    close.click();
    await settle(fixture);
    expect(ran).toEqual(['file.close']);
  });

  it('keeps the focused item bound to its own command when an item above it goes away (keyboard)', async () => {
    const file = (withExport: boolean): TopMenu[] => [
      {
        id: 'file',
        label: 'File',
        items: [
          ...(withExport ? [{ kind: 'item', id: 'report.export', label: 'Export report…' } as const] : []),
          { kind: 'item', id: 'file.close', label: 'Close workspace' },
          { kind: 'item', id: 'window.close', label: 'Close window' },
        ],
      },
    ];
    menuModel.set(file(true));
    await settle(fixture);
    const trigger = top('File');
    trigger.focus();
    press(trigger, 'ArrowDown');
    await settle(fixture);
    await vi.waitFor(() => expect(document.activeElement).toBe(item('Export report…')));
    press(document.activeElement!, 'ArrowDown');
    expect(document.activeElement).toBe(item('Close workspace'));
    menuModel.set(file(false));
    await settle(fixture);
    press(document.activeElement!, 'Enter');
    await settle(fixture);
    expect(ran).toEqual(['file.close']);
  });

  it('opens a submenu on click and runs its items', async () => {
    top('View').click();
    await settle(fixture);
    item('Analysis')!.click();
    await settle(fixture);
    expect(menus()).toEqual(['View', 'Analysis']);
    item('Anomalies')!.click();
    await settle(fixture);
    expect(ran).toEqual(['view:anomalies']);
    expect(menus()).toEqual([]);
  });

  it('is keyboard operable: arrows open and move, right/left open and close submenus, Escape closes one level', async () => {
    const view = top('View');
    view.focus();
    press(view, 'ArrowDown');
    await settle(fixture);
    expect(menus()).toEqual(['View']);
    await vi.waitFor(() => expect(document.activeElement).toBe(item('Timeline')));
    press(document.activeElement!, 'ArrowDown');
    expect(document.activeElement).toBe(item('Events'));
    press(document.activeElement!, 'ArrowDown');
    expect(document.activeElement).toBe(item('Analysis'));
    press(document.activeElement!, 'ArrowRight');
    await settle(fixture);
    expect(menus()).toEqual(['View', 'Analysis']);
    await vi.waitFor(() => expect(document.activeElement).toBe(item('Anomalies')));
    press(document.activeElement!, 'ArrowLeft');
    await settle(fixture);
    expect(menus()).toEqual(['View']);
    expect(document.activeElement).toBe(item('Analysis'));
    press(document.activeElement!, 'Escape');
    await settle(fixture);
    expect(menus()).toEqual([]);
    expect(document.activeElement).toBe(view);
  });

  it('jumps to an item by typing its first letters', async () => {
    const view = top('View');
    view.focus();
    press(view, 'ArrowDown');
    await settle(fixture);
    await vi.waitFor(() => expect(document.activeElement).toBe(item('Timeline')));
    press(document.activeElement!, 'o');
    await vi.waitFor(() => expect(document.activeElement).toBe(item('Other windows…')));
  });
});
