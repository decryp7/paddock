import { ChangeDetectionStrategy, Component, provideZonelessChangeDetection, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { afterEach, describe, expect, it } from 'vitest';
import { SuTrayPopover, type SuTrayMenuEntry } from './tray-popover';

type Id = 'customize' | 'release' | 'settings' | 'quit';

const MENU: readonly SuTrayMenuEntry<Id>[] = [
  { kind: 'separator' },
  { kind: 'item', id: 'customize', label: 'Customize widgets…', icon: 't:sliders' },
  { kind: 'item', id: 'release', label: 'Release memory', disabled: true },
  { kind: 'item', id: 'settings', label: 'Settings…', icon: 't:settings', keyHint: '⌘,' },
  { kind: 'separator' },
  { kind: 'separator' },
  { kind: 'item', id: 'quit', label: 'Quit Dyno', icon: 't:power', keyHint: null },
  { kind: 'separator' },
];

@Component({
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [SuTrayPopover],
  template: `
    <pd-tray-popover label="Dyno" [primaryLabel]="primary()" [menu]="menu()" [menuLabel]="menuLabel()" (primary)="opened = opened + 1" (menuAction)="actions.push($event)">
      <p>content</p>
      @if (extra()) {
        <button slot="footer" type="button" class="extra">Mark all seen</button>
      }
    </pd-tray-popover>
  `,
})
class Host {
  readonly primary = signal<string | null>('Open Dyno');
  readonly menu = signal<readonly SuTrayMenuEntry<Id>[]>(MENU);
  readonly menuLabel = signal('More actions');
  readonly extra = signal(false);
  opened = 0;
  readonly actions: Id[] = [];
}

describe('pd-tray-popover footer', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = '')));

  async function setup(change?: (h: Host) => void) {
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection()] });
    const icons = TestBed.inject(MatIconRegistry);
    const sanitizer = TestBed.inject(DomSanitizer);
    for (const n of ['sliders', 'settings', 'power']) icons.addSvgIconLiteral(`t:${n}`, sanitizer.bypassSecurityTrustHtml('<svg viewBox="0 0 24 24"></svg>'));
    const f = TestBed.createComponent(Host);
    change?.(f.componentInstance);
    document.body.appendChild(f.nativeElement);
    await f.whenStable();
    return f;
  }
  const footer = (f: { nativeElement: HTMLElement }) => f.nativeElement.querySelector<HTMLElement>('footer.pd-footer')!;
  const more = (f: { nativeElement: HTMLElement }) => footer(f).querySelector<HTMLButtonElement>('button.pd-tray-more');
  async function openMenu(f: Awaited<ReturnType<typeof setup>>): Promise<HTMLElement> {
    more(f)!.click();
    await f.whenStable();
    return document.querySelector<HTMLElement>('.mat-mdc-menu-panel')!;
  }

  it('draws the primary action as a real, right-aligned button with a decorative arrow, and emits primary', async () => {
    const f = await setup();
    const buttons = [...footer(f).querySelectorAll('button')];
    expect(buttons.map((b) => b.getAttribute('aria-label') ?? b.textContent!.trim())).toEqual(['Open Dyno', 'More actions']);
    const open = buttons[0]!;
    expect(open.type).toBe('button');
    expect(open.querySelector('pd-glyph')?.getAttribute('aria-hidden')).toBe('true');
    // The spacer comes first, so the actions sit at the right edge.
    expect(footer(f).firstElementChild?.classList).toContain('pd-footer-spacer');
    open.click();
    expect(f.componentInstance.opened).toBe(1);
  });

  it("keeps an app's own footer content at the start, before the spacer and the actions", async () => {
    const f = await setup((h) => h.extra.set(true));
    const kids = [...footer(f).children].map((c) => c.className.split(' ')[0]);
    expect(kids.slice(0, 2)).toEqual(['extra', 'pd-footer-spacer']);
  });

  it('names the more-actions trigger, and lists the items in order with a tidy separator, icons, key hints and disabled state', async () => {
    const f = await setup((h) => h.menuLabel.set('More Dyno actions'));
    expect(more(f)!.getAttribute('aria-label')).toBe('More Dyno actions');
    expect(more(f)!.getAttribute('aria-haspopup')).toBe('menu');
    const panel = await openMenu(f);
    const rows = [...panel.querySelector('.mat-mdc-menu-content')!.children].map((c) => (c.tagName === 'HR' ? '---' : c.textContent!.trim()));
    // The leading, doubled and trailing separators of the model are dropped; only the one between groups is drawn.
    expect(rows).toEqual(['Customize widgets…', 'Release memory', 'Settings…⌘,', '---', 'Quit Dyno']);
    const items = [...panel.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')];
    expect(items.map((i) => i.querySelector('mat-icon')?.getAttribute('data-mat-icon-name') ?? null)).toEqual(['sliders', null, 'settings', 'power']);
    expect(items[1]!.disabled).toBe(true);
    const kbd = items[2]!.querySelector('kbd')!;
    expect(kbd.getAttribute('aria-hidden')).toBe('true');
    expect(items[3]!.querySelector('kbd')).toBeNull();
    expect(panel.querySelector('hr')!.getAttribute('role')).toBe('separator');
  });

  it('marks the trigger while its menu is open, emits the chosen id and closes', async () => {
    const f = await setup();
    expect(more(f)!.classList).not.toContain('pd-tray-more--open');
    const panel = await openMenu(f);
    expect(more(f)!.classList).toContain('pd-tray-more--open');
    [...panel.querySelectorAll<HTMLButtonElement>('[role="menuitem"]')][2]!.click();
    await f.whenStable();
    expect(f.componentInstance.actions).toEqual(['settings']);
    expect(more(f)!.classList).not.toContain('pd-tray-more--open');
  });

  it('draws no trigger for an empty menu or one of separators only, and no actions without a label or menu', async () => {
    const f = await setup((h) => h.menu.set([{ kind: 'separator' }]));
    expect(more(f)).toBeNull();
    expect(footer(f).querySelector('button')?.textContent?.trim()).toBe('Open Dyno');
    f.componentInstance.primary.set(null);
    f.componentInstance.menu.set([]);
    await f.whenStable();
    expect(footer(f).children).toHaveLength(0);
  });

  it('keeps the icon gutter on an item without an icon when another item has one, so the labels align', async () => {
    const f = await setup();
    const panel = await openMenu(f);
    const items = [...panel.querySelectorAll<HTMLElement>('[role="menuitem"]')];
    // "Release memory" has no icon: a blank, hidden gutter of the icon's size stands in its place.
    const blank = items[1]!.querySelector('.pd-tray-icon--blank');
    expect(blank?.getAttribute('aria-hidden')).toBe('true');
    expect(blank?.classList).toContain('pd-tray-icon');
    expect(items.filter((i) => i.querySelector('.pd-tray-icon')).length).toBe(4);
  });

  it('draws no gutter when no item has an icon', async () => {
    const f = await setup((h) => h.menu.set([{ kind: 'item', id: 'settings', label: 'Settings…' }, { kind: 'item', id: 'quit', label: 'Quit' }]));
    const panel = await openMenu(f);
    expect(panel.querySelector('.pd-tray-icon')).toBeNull();
  });

  it('opens from the keyboard with the first item focused, and Escape gives the focus back to the trigger', async () => {
    const f = await setup();
    const trigger = more(f)!;
    trigger.focus();
    // A browser turns Enter on a button into a click; the trigger notes the keyboard first, so it focuses the first item.
    trigger.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', keyCode: 13, bubbles: true }));
    trigger.click();
    await f.whenStable();
    await new Promise((r) => setTimeout(r, 0));
    const panel = document.querySelector<HTMLElement>('.mat-mdc-menu-panel')!;
    expect(document.activeElement).toBe(panel.querySelector('[role="menuitem"]'));
    panel.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', keyCode: 27, bubbles: true }));
    await f.whenStable();
    await new Promise((r) => setTimeout(r, 0));
    expect(document.activeElement).toBe(trigger);
    expect(trigger.classList).not.toContain('pd-tray-more--open');
  });

  it('types the chosen id by the menu model (a wrong id does not compile)', () => {
    const entry: SuTrayMenuEntry<Id> = { kind: 'item', id: 'quit', label: 'Quit' };
    // @ts-expect-error: 'reboot' is not one of the model's ids.
    const wrong: SuTrayMenuEntry<Id> = { kind: 'item', id: 'reboot', label: 'Reboot' };
    expect([entry.kind, wrong.kind]).toEqual(['item', 'item']);
  });

  it('draws the menu alone when there is no primary action', async () => {
    const f = await setup((h) => h.primary.set(null));
    expect([...footer(f).querySelectorAll('button')].map((b) => b.getAttribute('aria-label'))).toEqual(['More actions']);
  });
});
