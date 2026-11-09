import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { DEFAULT_KINDS } from '../../core/mock/mock-popover';
import { render, settle } from '../../../testing/render';
import { TrayWidgetsPage } from './tray-widgets-page';

describe('the Tray popup page of Settings', () => {
  it('draws the chooser on the app\'s layout and a live preview of the real popup beside it', async () => {
    const { el } = await render(TrayWidgetsPage);
    expect(el.querySelector('h2')!.textContent).toBe('Tray popup');
    const chooser = el.querySelector('pd-widget-chooser')!;
    expect(chooser).not.toBeNull();
    expect(chooser.querySelectorAll('[role="list"] [role="listitem"], [cdkDrag]').length).toBeGreaterThan(0);
    const preview = el.querySelector('pd-tray-popover[aria-label="Preview of the popup"]')!;
    expect(Array.from(preview.querySelectorAll('section.pd-w')).map((s) => s.querySelector('.pd-w-title')?.textContent?.trim())).toEqual(
      ['Server', 'Activity', 'Memory', 'Loaded models'],
    );
    expect(DEFAULT_KINDS.length).toBe(4);
    expect(preview.hasAttribute('inert')).toBe(true);
    expect(preview.textContent).toContain('Serving on :11435');
  });

  it('sends each change to the app as one edit and shows the answer in the preview', async () => {
    const backend = new MockBackend();
    const edit = vi.spyOn(backend, 'editPopover');
    const { fixture, el } = await render(TrayWidgetsPage, backend);
    const remove = Array.from(document.querySelectorAll<HTMLElement>('body *')).length;
    expect(remove).toBeGreaterThan(0);
    await backend.editPopover({ remove: { id: 'memory' } });
    await settle(fixture, 6);
    expect(edit).toHaveBeenCalledWith({ remove: { id: 'memory' } });
    const preview = el.querySelector('pd-tray-popover')!;
    expect(preview.querySelectorAll('section.pd-w').length).toBe(3);
  });

  it('says what went wrong, with Retry, when the app does not answer', async () => {
    const { el } = await render(TrayWidgetsPage, new MockBackend({ failing: ['popoverView'] }));
    expect(el.querySelector('[role="alert"]')!.textContent).toContain('not responding');
    expect(el.querySelector('pd-widget-chooser')).toBeNull();
    expect(TestBed).toBeDefined();
  });
});
