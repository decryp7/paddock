import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import { PdChannelRemoveDialog } from './channel-remove-dialog';

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

describe('the channel remove dialog', () => {
  async function open(inputs: { channelId?: string; name?: string } = {}) {
    const closed: unknown[] = [];
    const titles: string[] = [];
    const ref: DialogRef<boolean> = { closed: Promise.resolve(undefined), close: (r) => void closed.push(r), setTitle: (t) => void titles.push(t) };
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: DIALOG_REF, useValue: ref }] });
    const fixture = TestBed.createComponent(PdChannelRemoveDialog);
    if (inputs.channelId !== undefined) fixture.componentRef.setInput('channelId', inputs.channelId);
    if (inputs.name !== undefined) fixture.componentRef.setInput('name', inputs.name);
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    const q = (id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);
    return { el, q, closed, titles };
  }

  it('names the channel, says the secret goes too and keeps the focus on Keep, never on Remove', async () => {
    const d = await open({ channelId: 'ops', name: 'Ops webhook' });
    expect(text(d.q('channel-remove-text')!)).toBe('Remove "Ops webhook"? Its stored secret is deleted too.');
    expect(d.titles.at(-1)).toBe('Remove "Ops webhook"?');
    const keep = d.q('channel-remove-keep')!;
    const remove = d.q('channel-remove-yes')!;
    expect(keep.hasAttribute('data-default')).toBe(true);
    expect(keep.hasAttribute('data-dialog-close')).toBe(true);
    expect(remove.hasAttribute('data-default')).toBe(false);
    expect(remove.className).toContain('pd-rm__btn--danger');
    expect(document.activeElement === keep || d.el.contains(document.activeElement)).toBe(true);
  });

  it('resolves true for Remove and false for Keep', async () => {
    const a = await open({ channelId: 'ops', name: 'Ops' });
    a.q('channel-remove-yes')!.click();
    expect(a.closed).toEqual([true]);
    TestBed.resetTestingModule();
    const b = await open({ channelId: 'ops', name: 'Ops' });
    b.q('channel-remove-keep')!.click();
    expect(b.closed).toEqual([false]);
  });

  it('falls back to the id when no name is given', async () => {
    const d = await open({ channelId: 'ops' });
    expect(text(d.q('channel-remove-text')!)).toContain('Remove "ops"?');
  });

  it('opened without a channel it says so inline with a Close button and offers no Remove', async () => {
    const d = await open();
    const note = d.q('channel-remove-invalid')!;
    expect(note.getAttribute('role')).toBe('alert');
    expect(text(note)).toContain('without a channel');
    expect(d.q('channel-remove-yes')).toBeNull();
    expect(d.titles).toEqual([]);
    d.q('channel-remove-close')!.click();
    expect(d.closed).toEqual([undefined]);
  });
});
