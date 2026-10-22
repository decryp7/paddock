import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { Toasts } from '../../core/toasts';
import { registerIcons } from '../icons';
import { ContextMenuService } from './context-menu';
import { DetailPanel } from './detail-panel';
import { Fact } from './fact';
import { RowMenu } from './row-menu';
import { fromControl, isMenuKey, moveRowFocus, rowTarget } from './row-nav';

@Component({
  imports: [DetailPanel, Fact, RowMenu],
  template: `
    <button id="opener" type="button">opener</button>
    @if (open()) {
      <pw-detail-panel heading="Qwen3" sub="repo" (closed)="open.set(false)">
        <pw-row-menu panel-actions [items]="items" label="More for Qwen3" />
        <button panel-toolbar type="button">Tool</button>
        <pw-fact label="Repository" value="a/b" [copy]="true" [mono]="true" />
        <pw-fact label="Size" value="5 GB" />
        <button panel-footer type="button">Foot</button>
      </pw-detail-panel>
    }
  `,
})
class Host {
  readonly open = signal(true);
  readonly ran: string[] = [];
  readonly items = [
    { label: 'One', run: () => this.ran.push('one') },
    { label: 'Delete…', run: () => this.ran.push('del'), danger: true, divider: true },
  ];
}

function setup() {
  TestBed.configureTestingModule({ imports: [Host] });
  registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
  const fixture = TestBed.createComponent(Host);
  document.body.appendChild(fixture.nativeElement);
  fixture.detectChanges();
  return fixture;
}

describe('row navigation helpers', () => {
  const rows = ['a', 'b', 'c'].map((id) => Object.assign(document.createElement('tr'), { id }));

  it('picks the row for arrow, Home and End keys and ignores others', () => {
    expect(rowTarget(rows, rows[0]!, 'ArrowDown')).toBe(rows[1]);
    expect(rowTarget(rows, rows[2]!, 'ArrowDown')).toBe(rows[2]);
    expect(rowTarget(rows, rows[1]!, 'ArrowUp')).toBe(rows[0]);
    expect(rowTarget(rows, rows[0]!, 'ArrowUp')).toBe(rows[0]);
    expect(rowTarget(rows, null, 'ArrowUp')).toBe(rows[0]);
    expect(rowTarget(rows, rows[1]!, 'Home')).toBe(rows[0]);
    expect(rowTarget(rows, rows[1]!, 'End')).toBe(rows[2]);
    expect(rowTarget(rows, rows[1]!, 'x')).toBeNull();
    expect(rowTarget([], null, 'ArrowDown')).toBeNull();
  });

  it('recognises the keyboard right-click', () => {
    expect(isMenuKey({ key: 'ContextMenu', shiftKey: false })).toBe(true);
    expect(isMenuKey({ key: 'F10', shiftKey: true })).toBe(true);
    expect(isMenuKey({ key: 'F10', shiftKey: false })).toBe(false);
  });

  it('moves focus only between rows and knows clicks that came from a control', () => {
    const table = document.createElement('table');
    const body = document.createElement('tbody');
    table.append(body);
    for (const r of rows) {
      r.tabIndex = 0;
      body.append(r);
    }
    document.body.append(table);
    const ev = (target: Element, key: string) => {
      const e = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true });
      Object.defineProperty(e, 'target', { value: target });
      return e;
    };
    expect(moveRowFocus(ev(rows[0]!, 'ArrowDown'))).toBe(true);
    expect(document.activeElement).toBe(rows[1]);
    expect(moveRowFocus(ev(rows[0]!, 'a'))).toBe(false);
    const cell = document.createElement('td');
    rows[0]!.append(cell);
    expect(moveRowFocus(ev(cell, 'ArrowDown'))).toBe(false);
    expect(moveRowFocus(ev(document.body, 'ArrowDown'))).toBe(false);
    const button = document.createElement('button');
    cell.append(button);
    expect(fromControl({ target: button } as unknown as Event)).toBe(true);
    expect(fromControl({ target: cell } as unknown as Event)).toBe(false);
    table.remove();
  });
});

describe('DetailPanel, Fact and RowMenu', () => {
  it('shows the heading, facts and slots, and closes with the button and with Escape', async () => {
    const fixture = setup();
    const el = fixture.nativeElement as HTMLElement;
    const panel = el.querySelector('pw-detail-panel')!;
    expect(panel.getAttribute('role')).toBe('complementary');
    expect(panel.getAttribute('aria-label')).toBe('Qwen3');
    expect(panel.textContent).toContain('Repository');
    expect(panel.textContent).toContain('Tool');
    expect(panel.textContent).toContain('Foot');
    panel.querySelector<HTMLButtonElement>('button[aria-label="Close details"]')!.click();
    fixture.detectChanges();
    expect(el.querySelector('pw-detail-panel')).toBeNull();
    fixture.componentInstance.open.set(true);
    fixture.detectChanges();
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    fixture.detectChanges();
    expect(el.querySelector('pw-detail-panel')).toBeNull();
    el.remove();
  });

  it('leaves Escape to an open menu and returns focus to the opener on close', () => {
    const fixture = setup();
    const el = fixture.nativeElement as HTMLElement;
    const opener = el.querySelector<HTMLElement>('#opener')!;
    fixture.componentInstance.open.set(false);
    fixture.detectChanges();
    opener.focus();
    fixture.componentInstance.open.set(true);
    fixture.detectChanges();
    const pane = document.createElement('div');
    pane.className = 'pw-menu';
    document.body.append(pane);
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    fixture.detectChanges();
    expect(el.querySelector('pw-detail-panel')).not.toBeNull();
    pane.remove();
    el.querySelector<HTMLElement>('pw-detail-panel button')!.focus();
    fixture.componentInstance.open.set(false);
    fixture.detectChanges();
    expect(document.activeElement).toBe(opener);
    el.remove();
  });

  it('copies a value with a toast, and says so when the clipboard refuses', async () => {
    const fixture = setup();
    const el = fixture.nativeElement as HTMLElement;
    const toasts = TestBed.inject(Toasts);
    const write = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { ...navigator, clipboard: { writeText: write } });
    el.querySelector<HTMLButtonElement>('button[aria-label="Copy repository"]')!.click();
    await Promise.resolve();
    expect(write).toHaveBeenCalledWith('a/b');
    expect(toasts.toasts().at(-1)?.message).toBe('Repository copied');
    write.mockRejectedValue(new Error('denied'));
    el.querySelector<HTMLButtonElement>('button[aria-label="Copy repository"]')!.click();
    await new Promise((r) => setTimeout(r, 0));
    expect(toasts.toasts().at(-1)?.tone).toBe('bad');
    expect(el.querySelectorAll('button[aria-label^="Copy"]').length).toBe(1);
    vi.unstubAllGlobals();
    el.remove();
  });

  it('opens the row menu from its button with a danger item and a divider, and runs an item', async () => {
    const fixture = setup();
    const el = fixture.nativeElement as HTMLElement;
    el.querySelector<HTMLButtonElement>('button[aria-label="More for Qwen3"]')!.click();
    fixture.detectChanges();
    const items = Array.from(document.querySelectorAll<HTMLButtonElement>('.pw-menu .pw-menu-item'));
    expect(items.map((b) => b.textContent!.trim())).toEqual(['One', 'Delete…']);
    expect(items[1]!.classList.contains('danger')).toBe(true);
    expect(document.querySelector('.pw-menu hr')).not.toBeNull();
    items[1]!.click();
    expect(fixture.componentInstance.ran).toEqual(['del']);
    TestBed.inject(ContextMenuService).close();
    el.remove();
  });
});
