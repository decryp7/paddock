import { TestBed } from '@angular/core/testing';
import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { PdChannelRemoveDialog } from './channel-remove-dialog';

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();
const open = (inputs: { channelId?: string; name?: string } = {}) => renderFramed(PdChannelRemoveDialog, { id: 'pd.channels.remove', inputs });

describe('the channel remove dialog', () => {
  it('names the channel and says the secret goes too; Keep has the focus and Enter, Remove channel never does', async () => {
    const d = await open({ channelId: 'ops', name: 'Ops webhook' });
    expect(d.heading()).toBe('Remove the channel “Ops webhook”?');
    expect(text(d.q('channel-remove-text')!)).toBe('Its stored secret is deleted too.');
    // The in-page dialog is the alertdialog, named by its question and described by its message; no nested roles.
    expect(d.el.getAttribute('role')).toBe('alertdialog');
    expect(d.el.getAttribute('aria-labelledby')).toBe(d.q('dialog-heading')!.id);
    expect(d.el.getAttribute('aria-describedby')).toBe(d.q('channel-remove-text')!.id);
    expect(d.q('dialog-body')!.getAttribute('role')).toBeNull();
    expect(d.q('dialog-icon')!.dataset['tone']).toBe('danger');
    expect(d.actions()).toContain('keep');
    expect(d.action('remove')!.textContent!.trim()).toBe('Remove channel');
    expect(d.action('remove')!.className).toContain('pd-frame-btn--danger');
    expect(document.activeElement).toBe(d.action('keep'));
    // Enter outside a control runs the Enter target: Keep (a button keeps its own Enter).
    d.action('keep')!.blur();
    await d.key('Enter');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('answers true for Remove channel and is dismissed by Keep', async () => {
    const a = await open({ channelId: 'ops', name: 'Ops' });
    await a.press('remove');
    expect(a.outcome()).toEqual({ kind: 'answered', value: true });
    TestBed.resetTestingModule();
    const b = await open({ channelId: 'ops', name: 'Ops' });
    await b.press('keep');
    expect(b.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('falls back to the id when no name is given', async () => {
    const d = await open({ channelId: 'ops' });
    expect(d.heading()).toBe('Remove the channel “ops”?');
  });

  it('opened without a channel it says so inline with a Close and offers no Remove', async () => {
    const d = await open();
    const note = d.q('channel-remove-invalid')!;
    expect(note.getAttribute('role')).toBe('alert');
    expect(text(note)).toContain('without a channel');
    expect(d.actions()).toEqual(['close']);
    await d.press('close');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
  });
});
