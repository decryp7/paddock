import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MemoryMcpBackend, provideMcpBackend, type MemoryMcpOptions } from '@decryp7/paddock-ui/shell/mcp';
import { DIALOG_REF, type DialogRef } from '@decryp7/paddock-ui/shell/dialogs';
import { PdMcpKeyDialog } from './mcp-key-dialog';
import { PdMcpRevokeDialog } from './mcp-revoke-dialog';

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

function ref() {
  const closed: unknown[] = [];
  const titles: string[] = [];
  const dialogRef: DialogRef<boolean> = { closed: Promise.resolve(undefined), close: (r) => void closed.push(r), setTitle: (t) => void titles.push(t) };
  return { closed, titles, dialogRef };
}

describe('the key dialog', () => {
  async function open(options: MemoryMcpOptions = {}) {
    const memory = new MemoryMcpBackend(options);
    const r = ref();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideMcpBackend({ backend: () => memory }), { provide: DIALOG_REF, useValue: r.dialogRef }] });
    const fixture = TestBed.createComponent(PdMcpKeyDialog);
    await fixture.whenStable();
    const el = fixture.nativeElement as HTMLElement;
    const q = (id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);
    const flush = async (): Promise<void> => {
      await settle();
      await fixture.whenStable();
    };
    const type = async (value: string): Promise<void> => {
      const input = q('mcp-key-client') as HTMLInputElement;
      input.value = value;
      input.dispatchEvent(new Event('input', { bubbles: true }));
      await flush();
    };
    const submit = async (): Promise<void> => {
      q('mcp-key-form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
      await flush();
    };
    return { fixture, el, q, type, submit, flush, memory, ...r };
  }

  it('asks for the client’s name first and cannot create without one', async () => {
    const d = await open();
    expect((d.q('mcp-key-client') as HTMLInputElement).getAttribute('autocomplete')).toBe('off');
    expect((d.q('mcp-key-create') as HTMLButtonElement).disabled).toBe(true);
    await d.submit();
    expect(d.memory.issued).toEqual([]);
    await d.type('   ');
    expect((d.q('mcp-key-create') as HTMLButtonElement).disabled).toBe(true);
    await d.type('Pitwall');
    expect((d.q('mcp-key-create') as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows the key once, copies it, and closes with true on Done', async () => {
    const d = await open();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await d.type('Pitwall');
    await d.submit();
    const shown = d.q('mcp-key-shown');
    expect(shown).not.toBeNull();
    const key = (d.q('mcp-key-value') as HTMLInputElement).value;
    expect(key).toBe(d.memory.issued[0]);
    expect(text(shown!)).toContain('only time the key is shown');
    expect(d.q('mcp-key-form')).toBeNull();
    d.q('mcp-key-copy')!.click();
    await d.flush();
    expect(writeText).toHaveBeenCalledWith(key);
    expect(text(d.q('mcp-key-copy-status')!)).toContain('Copied');
    d.q('mcp-key-done')!.click();
    await d.flush();
    expect(d.closed).toEqual([true]);
    expect(d.q('mcp-key-value')).toBeNull();
    expect(d.el.innerHTML).not.toContain(key);
  });

  it('says so when the clipboard is not available and still shows the key to select', async () => {
    const d = await open();
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('denied')) }, configurable: true });
    await d.type('x');
    await d.submit();
    d.q('mcp-key-copy')!.click();
    await d.flush();
    expect(text(d.q('mcp-key-copy-status')!)).toContain('clipboard is not available');
    expect((d.q('mcp-key-value') as HTMLInputElement).readOnly).toBe(true);
    (d.q('mcp-key-value') as HTMLInputElement).dispatchEvent(new Event('focus'));
  });

  it('shows the engine’s refusal inline and lets the user try again', async () => {
    const d = await open({ issueError: { code: 'invalid', message: '16 keys are in use. Revoke one you no longer need first.', retryable: false } });
    await d.type('Pitwall');
    await d.submit();
    expect(text(d.q('mcp-key-error')!)).toContain('16 keys are in use');
    expect(d.q('mcp-key-shown')).toBeNull();
    expect((d.q('mcp-key-create') as HTMLButtonElement).disabled).toBe(false);
    expect(d.closed).toEqual([]);
  });

  it('closes with nothing on Cancel and never issues a key', async () => {
    const d = await open();
    d.q('mcp-key-cancel')!.click();
    expect(d.closed).toEqual([undefined]);
    expect(d.memory.issued).toEqual([]);
  });
});

describe('the revoke dialog', () => {
  function open(inputs: { keyId?: number; client?: string } = {}) {
    const r = ref();
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), { provide: DIALOG_REF, useValue: r.dialogRef }] });
    const fixture = TestBed.createComponent(PdMcpRevokeDialog);
    if (inputs.keyId !== undefined) fixture.componentRef.setInput('keyId', inputs.keyId);
    if (inputs.client !== undefined) fixture.componentRef.setInput('client', inputs.client);
    return fixture.whenStable().then(() => {
      const el = fixture.nativeElement as HTMLElement;
      return { el, q: (id: string) => el.querySelector(`[data-testid="${id}"]`) as HTMLElement | null, fixture, ...r };
    });
  }

  it('names the client, makes Keep the default and the title the question', async () => {
    const d = await open({ keyId: 3, client: 'Pitwall' });
    expect(text(d.q('mcp-revoke-text')!)).toContain('Revoke the key of "Pitwall"?');
    expect(text(d.q('mcp-revoke-text')!)).toContain('stops working at once');
    expect(d.q('mcp-revoke-keep')!.hasAttribute('data-default')).toBe(true);
    expect(d.q('mcp-revoke-yes')!.hasAttribute('data-default')).toBe(false);
    expect(d.titles.at(-1)).toBe('Revoke the key of "Pitwall"?');
  });

  it('closes with true on Revoke and false on Keep', async () => {
    const d = await open({ keyId: 3, client: 'Pitwall' });
    d.q('mcp-revoke-yes')!.click();
    d.q('mcp-revoke-keep')!.click();
    expect(d.closed).toEqual([true, false]);
  });

  it('says so, with a Close button, when it was opened without a key', async () => {
    const d = await open();
    expect(text(d.q('mcp-revoke-invalid')!)).toContain('without a key');
    expect(d.q('mcp-revoke-yes')).toBeNull();
    d.q('mcp-revoke-close')!.click();
    expect(d.closed).toEqual([undefined]);
  });

  it('names the key by its number when the client is unknown', async () => {
    const d = await open({ keyId: 9 });
    expect(text(d.q('mcp-revoke-text')!)).toContain('"9"');
  });
});
