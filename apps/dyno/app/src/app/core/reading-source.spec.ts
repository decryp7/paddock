import { TestBed } from '@angular/core/testing';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { SETTINGS_DIALOG_ID } from '@decryp7/paddock-ui/shell/settings';
import type { ReadVia } from './api.types';
import { HELPER_CHIP, ReadingSource, TURN_ON_LABEL, sourceView } from './reading-source';

const NEEDS: ReadVia = { needsHelper: { reason: "Other accounts' processes are not readable. Turn on Administrator readings in Settings so the helper can read this.", next: { openSettings: 'helper' } } };

function render(via: ReadVia | undefined, hint = '') {
  const open = vi.fn().mockResolvedValue(undefined);
  TestBed.configureTestingModule({ providers: [{ provide: DialogService, useValue: { open } }] });
  const f = TestBed.createComponent(ReadingSource);
  f.componentRef.setInput('via', via);
  f.componentRef.setInput('hint', hint);
  f.detectChanges();
  return { el: f.nativeElement as HTMLElement, open };
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
    expect(sourceView(NEEDS)).toMatchObject({ kind: 'needsHelper', settings: true });
    expect(sourceView({ needsHelper: { reason: 'x', next: 'retry' } })).toMatchObject({ kind: 'needsHelper', settings: false });
    expect(sourceView({ needsHelper: { reason: 'x', next: { openSettings: 'general' } } })).toMatchObject({ settings: false });
    expect(sourceView({ unavailable: { reason: 'No sensors.' } })).toEqual({ kind: 'unavailable', reason: 'No sensors.' });
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
    expect(q(el, 'reading-source-needs')?.textContent).toContain('Turn on Administrator readings in Settings');
    const button = q(el, 'reading-source-turn-on') as HTMLButtonElement;
    expect(button.textContent?.trim()).toBe(TURN_ON_LABEL);
    button.click();
    expect(open).toHaveBeenCalledWith(SETTINGS_DIALOG_ID, { inputs: { page: 'dy.general', anchor: { id: 'helper', at: expect.any(Number) } } });
  });

  it('shows only the reason, no button, when neither path can read it', () => {
    const { el } = render({ unavailable: { reason: 'This virtual machine has no sensors.' } });
    expect(q(el, 'reading-source-unavailable')?.textContent).toContain('no sensors');
    expect(el.querySelector('button')).toBeNull();
  });
});
