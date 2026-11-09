import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { BACKEND } from '../core/backend';
import { MockBackend } from '../core/mock-backend';
import { WINDOW_CONTROL } from '../core/window-control';
import { TrayPopover } from '../features/tray/tray-popover';
import { registerIcons } from '../shared/icons';
import { settle } from '../../testing/render';
import { TRAY_BACKEND_CALLS } from './tray-window';

describe('the tray popover window', () => {
  it('boots and works with only the calls open to the TRAY class', async () => {
    const mock = new MockBackend();
    // Records every method the window calls on its backend (the mock's own inner calls are not the window's).
    const called = new Set<string>();
    const backend = new Proxy(mock, {
      get(target, name, receiver) {
        const value = Reflect.get(target, name, receiver) as unknown;
        if (typeof value !== 'function' || typeof name !== 'string') return value;
        return (...args: unknown[]) => {
          called.add(name);
          return (value as (...a: unknown[]) => unknown).apply(target, args);
        };
      },
    });
    const windows = { openMain: async () => undefined, onNavigate: () => () => undefined, fitTray: async () => undefined, closeTray: async () => undefined, quit: async () => undefined };
    TestBed.configureTestingModule({ imports: [TrayPopover], providers: [{ provide: BACKEND, useValue: backend }, { provide: WINDOW_CONTROL, useValue: windows }] });
    registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    const fixture = TestBed.createComponent(TrayPopover);
    await settle(fixture);
    const el = fixture.nativeElement as HTMLElement;
    (el.querySelector('button[aria-label="More actions"]') as HTMLButtonElement).click();
    await settle(fixture);
    for (const label of ['Pause serving', 'Release memory']) {
      Array.from(document.querySelectorAll<HTMLButtonElement>('.mat-mdc-menu-item'))
        .find((b) => b.textContent!.includes(label))!
        .click();
      await settle(fixture);
      (el.querySelector('button[aria-label="More actions"]') as HTMLButtonElement).click();
      await settle(fixture);
    }
    (el.querySelector('pd-widget-area .row') as HTMLButtonElement).click();
    for (const call of ['popoverView', 'popoverValues', 'popoverHeader', 'popoverShape', 'onPopover', 'setServing', 'releaseMemory']) expect(called.has(call), call).toBe(true);
    expect([...called].filter((c) => !(TRAY_BACKEND_CALLS as readonly string[]).includes(c))).toEqual([]);
  });
});
