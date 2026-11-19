import { ApplicationRef } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { describe, expect, it } from 'vitest';
import { ContextMenuHost } from './context-menu-host';
import { ContextMenuService } from './context-menu.service';

describe('ContextMenuHost', () => {
  function setup() {
    const fixture = TestBed.createComponent(ContextMenuHost);
    const service = TestBed.inject(ContextMenuService);
    const menu = () => (fixture.nativeElement as HTMLElement).querySelector<HTMLElement>('[role=menu]');
    const render = () => TestBed.inject(ApplicationRef).tick();
    return { fixture, service, menu, render };
  }

  it('says when the menu is in its place (data-placed), after the render that keeps it inside the window, and again for each opening', () => {
    const { fixture, service, menu, render } = setup();
    service.open(40, 50, [{ label: 'Copy', run: () => undefined }], null);
    // Drawn (the view's own change detection, no render hooks) but not measured yet: not placed.
    fixture.componentRef.changeDetectorRef.detectChanges();
    expect(menu()).not.toBeNull();
    expect(menu()?.getAttribute('data-placed')).toBeNull();
    render();
    expect(menu()?.getAttribute('data-placed')).toBe('true');
    // Opened again elsewhere while open: placed for the new opening once its render has run.
    service.open(60, 70, [{ label: 'Paste', run: () => undefined }], null);
    render();
    expect(menu()?.style.left).toBe('60px');
    expect(menu()?.getAttribute('data-placed')).toBe('true');
    service.close(false);
    render();
    expect(menu()).toBeNull();
  });
});
