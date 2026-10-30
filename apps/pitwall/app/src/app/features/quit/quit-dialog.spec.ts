import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { QuitDialog } from './quit-dialog';

const open = () => renderFramed(QuitDialog, { id: 'pw.quit', kind: 'decision', modality: 'modal' });

describe('QuitDialog (pw.quit)', () => {
  afterEach(() => delete document.documentElement.dataset['platform']);

  it('asks "Quit Pitwall?" with Cancel, Quit and stop serving, and Quit, keep serving in the native order of each OS', async () => {
    document.documentElement.dataset['platform'] = 'macos';
    const mac = await open();
    expect(mac.heading()).toBe('Quit Pitwall?');
    expect(mac.actions()).toEqual(['stop', 'cancel', 'keep']);
    expect(mac.action('keep')!.className).toContain('pd-frame-btn--filled');
    expect(mac.action('stop')!.className).not.toContain('danger');
    mac.handle.dismiss();
    await mac.stable();
    document.documentElement.dataset['platform'] = 'windows';
    const { TestBed } = await import('@angular/core/testing');
    TestBed.resetTestingModule();
    const win = await open();
    expect(win.actions()).toEqual(['keep', 'stop', 'cancel']);
  });

  it('the safe answer has the focus and Enter: quit, keep serving, not remembered', async () => {
    const d = await open();
    expect(document.activeElement).toBe(d.action('keep'));
    d.action('keep')!.blur();
    await d.key('Enter');
    expect(d.outcome()).toEqual({ kind: 'answered', value: { stopServing: false, remember: false } });
  });

  it('answers stop serving and remembers it when "Don\'t ask again" is ticked', async () => {
    const d = await open();
    d.q('quit-remember')!.querySelector<HTMLInputElement>('input')!.click();
    await d.stable();
    await d.press('stop');
    expect(d.outcome()).toEqual({ kind: 'answered', value: { stopServing: true, remember: true } });
  });

  it('Cancel and Escape answer nothing', async () => {
    const d = await open();
    await d.press('cancel');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
    const { TestBed } = await import('@angular/core/testing');
    TestBed.resetTestingModule();
    const e = await open();
    await e.key('Escape');
    expect(e.outcome()).toEqual({ kind: 'dismissed' });
  });
});
