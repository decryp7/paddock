import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MemoryChannelsBackend, provideChannelsBackend } from '@decryp7/paddock-ui/shell/channels';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import { PdChannelSecretDialog } from './channel-secret-dialog';

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

describe('the channel secret dialog', () => {
  async function open(inputs: { channelId?: string; label?: string; kind?: 'token' | 'password' } = {}, memory = new MemoryChannelsBackend()) {
    const closed: unknown[] = [];
    const titles: string[] = [];
    const ref: DialogRef<boolean> = { closed: Promise.resolve(undefined), close: (r) => void closed.push(r), setTitle: (t) => void titles.push(t) };
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideChannelsBackend({ backend: () => memory }), { provide: DIALOG_REF, useValue: ref }],
    });
    const fixture = TestBed.createComponent(PdChannelSecretDialog);
    if (inputs.channelId !== '') fixture.componentRef.setInput('channelId', inputs.channelId ?? 'slack');
    if (inputs.label !== undefined) fixture.componentRef.setInput('label', inputs.label);
    if (inputs.kind) fixture.componentRef.setInput('kind', inputs.kind);
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    const q = (id: string): HTMLElement => el.querySelector(`[data-testid="${id}"]`) as HTMLElement;
    const input = (): HTMLInputElement => q('channel-secret-input') as HTMLInputElement;
    const flush = async (): Promise<void> => {
      await settle();
      await fixture.whenStable();
    };
    const typeSecret = async (value: string): Promise<void> => {
      input().value = value;
      input().dispatchEvent(new Event('input', { bubbles: true }));
      await flush();
    };
    const submit = async (): Promise<void> => {
      q('channel-secret-form').dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      await flush();
    };
    return { el, q, input, typeSecret, submit, flush, closed, titles, memory };
  }

  it('asks for a token with a masked field that does not autocomplete, and names the channel in the title', async () => {
    const d = await open({ label: 'Team Slack' });
    expect(d.input().type).toBe('password');
    expect(d.input().getAttribute('autocomplete')).toBe('off');
    expect(d.input().getAttribute('spellcheck')).toBe('false');
    expect(text(d.el.querySelector('label')!)).toContain('Token');
    expect(text(d.el)).toContain('The token for Team Slack');
    expect(d.titles.at(-1)).toBe('Token for Team Slack');
    expect((d.q('channel-secret-save') as HTMLButtonElement).disabled).toBe(true);
    expect(d.q('channel-secret-cancel').hasAttribute('data-dialog-close')).toBe(true);
    expect(d.q('channel-secret-save').className).toContain('primary');
  });

  it('asks for a password for an email channel, falling back to the id as the name', async () => {
    const d = await open({ kind: 'password', channelId: 'mail' });
    expect(text(d.el.querySelector('label')!)).toContain('Password');
    expect(d.titles.at(-1)).toBe('Password for mail');
  });

  it('shows and hides what was typed', async () => {
    const d = await open();
    const toggle = d.q('channel-secret-toggle');
    expect(toggle.getAttribute('aria-pressed')).toBe('false');
    expect(toggle.getAttribute('aria-label')).toBe('Show what you typed');
    toggle.click();
    await d.flush();
    expect(d.input().type).toBe('text');
    expect(toggle.getAttribute('aria-pressed')).toBe('true');
    expect(toggle.getAttribute('aria-label')).toBe('Hide what you typed');
  });

  it('sends the secret straight to the engine, clears it and resolves true', async () => {
    const d = await open();
    await d.typeSecret('s3cret');
    expect((d.q('channel-secret-save') as HTMLButtonElement).disabled).toBe(false);
    await d.submit();
    expect(d.memory.secretOf('slack')).toBe('s3cret');
    expect(d.closed).toEqual([true]);
    expect(d.input().value).toBe('');
    expect(d.el.innerHTML).not.toContain('s3cret');
  });

  it('shows the engine’s refusal, keeps the dialog open and the secret for another try', async () => {
    class Refusing extends MemoryChannelsBackend {
      override setSecret(): Promise<void> {
        return Promise.reject({ code: 'storeUnavailable', message: 'The keychain is locked.', retryable: true });
      }
    }
    const d = await open({}, new Refusing());
    await d.typeSecret('s3cret');
    await d.submit();
    expect(d.closed).toEqual([]);
    const error = d.q('channel-secret-error');
    expect(error.getAttribute('role')).toBe('alert');
    expect(text(error)).toBe('The keychain is locked.');
    expect(d.input().getAttribute('aria-invalid')).toBe('true');
    expect(d.input().value).toBe('s3cret');
    expect((d.q('channel-secret-save') as HTMLButtonElement).disabled).toBe(false);
  });

  it('opened without a channel it says so inline with a Close button, and shows no field to type a secret in', async () => {
    const d = await open({ channelId: '' });
    const note = d.el.querySelector('[data-testid="channel-secret-invalid"]')!;
    expect(note.getAttribute('role')).toBe('alert');
    expect(text(note)).toContain('without a channel');
    expect(d.el.querySelector('input')).toBeNull();
    expect(d.titles).toEqual([]);
    const close = d.el.querySelector('[data-testid="channel-secret-close"]') as HTMLElement;
    expect(close.hasAttribute('data-dialog-close')).toBe(true);
    close.click();
    expect(d.closed).toEqual([undefined]);
    expect(d.memory.secretOf('')).toBeUndefined();
  });

  it('does nothing on Enter without a secret, and Cancel resolves nothing', async () => {
    const d = await open();
    await d.submit();
    expect(d.closed).toEqual([]);
    expect(d.memory.secretOf('slack')).toBeUndefined();
    d.q('channel-secret-cancel').click();
    expect(d.closed).toEqual([undefined]);
  });
});
