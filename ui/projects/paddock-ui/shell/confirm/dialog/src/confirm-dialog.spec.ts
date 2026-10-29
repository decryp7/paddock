import { TestBed } from '@angular/core/testing';
import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { CONFIRM_DIALOG_CONTRACTS, CONFIRM_DIALOG_ID, confirm, defineConfirmDialog, type ConfirmInput } from '@decryp7/paddock-ui/shell/confirm';
import { PdConfirmDialog } from './confirm-dialog';

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();
const open = (inputs: Partial<ConfirmInput> = {}) => renderFramed(PdConfirmDialog, { id: CONFIRM_DIALOG_ID, inputs });

describe('pd.confirm', () => {
  it('is a modal decision, one window per question, that calls nothing', () => {
    const def = defineConfirmDialog({ load: () => Promise.resolve(PdConfirmDialog) });
    expect([def.id, def.kind, def.modality, def.instanced, def.width, def.commands]).toEqual(['pd.confirm', 'decision', 'modal', true, 400, []]);
    expect(CONFIRM_DIALOG_CONTRACTS['pd.confirm'].requiredChoice).toBe(false);
  });

  it('asks a destructive question: the danger action is never the focus or the Enter target', async () => {
    const d = await open({ heading: 'Delete the watch?', message: 'Its history goes too.', details: ['Downloads', '3 rules'], confirm: { label: 'Delete watch', destructive: true } });
    expect(d.heading()).toBe('Delete the watch?');
    expect(d.q('dialog-icon')!.dataset['tone']).toBe('danger');
    expect(text(d.q('confirm-message')!)).toBe('Its history goes too.');
    expect([...d.q('confirm-details')!.querySelectorAll('li')].map(text)).toEqual(['Downloads', '3 rules']);
    expect(d.action('confirm')!.className).toContain('pd-frame-btn--danger');
    expect(document.activeElement).toBe(d.action('cancel'));
    d.action('cancel')!.blur();
    await d.key('Enter');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('answers true for a primary affirmative on Enter, with the opener’s Cancel label', async () => {
    const d = await open({ heading: 'Download version 2.0?', message: 'It downloads in the background.', confirm: { label: 'Download' }, cancelLabel: 'Not now' });
    expect(d.action('cancel')!.textContent!.trim()).toBe('Not now');
    expect(d.action('confirm')!.className).toContain('pd-frame-btn--filled');
    expect(d.q('confirm-details')).toBeNull();
    expect(document.activeElement).toBe(d.action('confirm'));
    d.action('confirm')!.blur();
    await d.key('Enter');
    expect(d.outcome()).toEqual({ kind: 'answered', value: true });
  });

  it('says so, with Close only, when opened without a question', async () => {
    const d = await open({ message: 'x' });
    expect(d.q('confirm-invalid')!.getAttribute('role')).toBe('alert');
    expect(d.actions()).toEqual(['close']);
    TestBed.resetTestingModule();
    const e = await open({ heading: 'H?', message: 'm', confirm: { label: ' ' } });
    expect(e.actions()).toEqual(['close']);
  });

  it('confirm() opens pd.confirm with the question and tells the answer from a dismissal', async () => {
    const opened: unknown[] = [];
    const answer = (kind: 'answered' | 'dismissed') => ({ open: (id: string, o: unknown) => (opened.push([id, o]), { closed: Promise.resolve({ kind }) }) });
    const input: ConfirmInput = { heading: 'Remove?', message: 'm', confirm: { label: 'Remove channel', destructive: true } };
    expect(await confirm(answer('answered'), input)).toBe(true);
    expect(await confirm(answer('dismissed'), input)).toBe(false);
    expect(opened[0]).toEqual(['pd.confirm', { inputs: input, title: 'Remove channel' }]);
  });
});
