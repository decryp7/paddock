import { TestBed } from '@angular/core/testing';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { SETTINGS_DIALOG_ID } from '@decryp7/paddock-ui/shell/settings';
import type { ReadVia } from './api.types';
import { HELPER_CHIP, OPEN_SETTINGS_LABEL, ReadingSource, RETRY_LABEL, TURN_ON_LABEL, UPDATE_LABEL, sourceView } from './reading-source';

const NEEDS: ReadVia = { needsHelper: { reason: "Other accounts' processes are not readable.", next: { openSettings: 'helper' } } };

function render(via: ReadVia | undefined, hint = '', canRetry = false) {
  const open = vi.fn().mockResolvedValue(undefined);
  TestBed.configureTestingModule({ providers: [{ provide: DialogService, useValue: { open } }] });
  const f = TestBed.createComponent(ReadingSource);
  f.componentRef.setInput('via', via);
  f.componentRef.setInput('hint', hint);
  f.componentRef.setInput('canRetry', canRetry);
  const retried = vi.fn();
  f.componentInstance.retry.subscribe(retried);
  f.detectChanges();
  return { el: f.nativeElement as HTMLElement, open, retried };
}
const q = (el: HTMLElement, id: string) => el.querySelector(`[data-testid="${id}"]`);

afterEach(() => TestBed.resetTestingModule());

describe('sourceView', () => {
  it('shows nothing for a plain local reading and a hint only when the caller gives one', () => {
    expect(sourceView('local')).toEqual({ kind: 'none' });
    expect(sourceView(undefined)).toEqual({ kind: 'none' });
    expect(sourceView('local', 'More with the helper.')).toEqual({ kind: 'hint', text: 'More with the helper.' });
  });

  it('names the helper, and gives a needs-helper reason a Settings step only for Settings > Helper', () => {
    expect(sourceView('helper')).toEqual({ kind: 'helper', text: HELPER_CHIP });
    expect(sourceView(NEEDS)).toMatchObject({ kind: 'needsHelper', step: 'turnOn' });
    expect(sourceView({ needsHelper: { reason: 'x', next: 'retry' } })).toMatchObject({ kind: 'needsHelper', step: null });
    expect(sourceView({ needsHelper: { reason: 'x', next: { openSettings: 'general' } } })).toMatchObject({ step: null });
    expect(sourceView({ needsHelper: { reason: 'x', next: 'updateHelper' } })).toMatchObject({ step: 'update' });
    expect(sourceView({ unavailable: { reason: 'No sensors.', next: null } })).toEqual({ kind: 'unavailable', reason: 'No sensors.', step: null });
    expect(sourceView({ unavailable: { reason: 'x', next: 'retry' } })).toMatchObject({ step: 'retry' });
    expect(sourceView({ unavailable: { reason: 'x', next: { openSettings: 'helper' } } })).toMatchObject({ step: 'settings' });
    expect(sourceView({ unavailable: { reason: 'x', next: 'showLog' } })).toMatchObject({ step: null });
  });
});

describe('dy-reading-source', () => {
  it('shows a neutral chip with words when the helper read it', () => {
    const { el } = render('helper');
    expect(q(el, 'reading-source-helper')?.textContent?.trim()).toBe('Read by the administrator helper');
    expect(el.querySelector('button')).toBeNull();
  });

  it('shows nothing for a local reading without a hint', () => {
    expect(render('local').el.textContent?.trim()).toBe('');
  });

  it('shows the reason and a verb-plus-object button that opens Settings at the helper card', () => {
    const { el, open } = render(NEEDS);
    expect(q(el, 'reading-source-needs')?.textContent).toContain('not readable');
    const button = q(el, 'reading-source-turn-on') as HTMLButtonElement;
    expect(button.textContent?.trim()).toBe(TURN_ON_LABEL);
    button.click();
    expect(open).toHaveBeenCalledWith(SETTINGS_DIALOG_ID, { inputs: { page: 'dy.general', anchor: { id: 'helper', at: expect.any(Number) } } });
  });

  it('offers to update an outdated helper in the same place', () => {
    const { el, open } = render({ needsHelper: { reason: 'The helper is older than this Dyno accepts.', next: 'updateHelper' } });
    const button = q(el, 'reading-source-turn-on') as HTMLButtonElement;
    expect(button.textContent?.trim()).toBe(UPDATE_LABEL);
    button.click();
    expect(open).toHaveBeenCalledTimes(1);
  });

  it('offers Settings when the helper needs repairing, as the next step says', () => {
    const { el, open } = render({ unavailable: { reason: 'Another program answers where the helper should be.', next: { openSettings: 'helper' } } });
    const button = q(el, 'reading-source-step') as HTMLButtonElement;
    expect(button.textContent?.trim()).toBe(OPEN_SETTINGS_LABEL);
    button.click();
    expect(open).toHaveBeenCalledWith(SETTINGS_DIALOG_ID, { inputs: { page: 'dy.general', anchor: { id: 'helper', at: expect.any(Number) } } });
  });

  it('offers Try again when the helper did not answer, and tells the page to read again', () => {
    const { el, open, retried } = render({ unavailable: { reason: 'The helper did not answer.', next: 'retry' } }, '', true);
    const button = q(el, 'reading-source-step') as HTMLButtonElement;
    expect(button.textContent?.trim()).toBe(RETRY_LABEL);
    button.click();
    expect(retried).toHaveBeenCalledTimes(1);
    expect(open).not.toHaveBeenCalled();
  });

  it('shows no Try again button unless the page says it can read again', () => {
    const { el } = render({ unavailable: { reason: 'The helper did not answer.', next: 'retry' } });
    expect(q(el, 'reading-source-unavailable')?.textContent).toContain('did not answer');
    expect(q(el, 'reading-source-step')).toBeNull();
  });

  it('shows only the reason, no button, when neither path can read it', () => {
    const { el } = render({ unavailable: { reason: 'This virtual machine has no sensors.', next: null } });
    expect(q(el, 'reading-source-unavailable')?.textContent).toContain('no sensors');
    expect(el.querySelector('button')).toBeNull();
  });
});
