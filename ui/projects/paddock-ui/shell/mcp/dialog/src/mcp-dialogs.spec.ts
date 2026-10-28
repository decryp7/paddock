import { TestBed } from '@angular/core/testing';
import { MemoryMcpBackend, provideMcpBackend, type MemoryMcpOptions } from '@decryp7/paddock-ui/shell/mcp';
import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { PdMcpKeyDialog } from './mcp-key-dialog';
import { PdMcpRevokeDialog } from './mcp-revoke-dialog';

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

describe('the key dialog', () => {
  async function open(options: MemoryMcpOptions = {}) {
    const memory = new MemoryMcpBackend(options);
    const d = await renderFramed(PdMcpKeyDialog, { id: 'pd.mcp.key', kind: 'form', modality: 'modal', providers: [provideMcpBackend({ backend: () => memory })] });
    const type = async (value: string): Promise<void> => {
      const input = d.q<HTMLInputElement>('mcp-key-client')!;
      input.value = value;
      input.dispatchEvent(new Event('input', { bubbles: true }));
      await d.stable();
    };
    return { ...d, type, memory, el: d.el };
  }

  it('asks for the client’s name first, with the focus in it, and cannot create without one', async () => {
    const d = await open();
    expect(d.q<HTMLInputElement>('mcp-key-client')!.getAttribute('autocomplete')).toBe('off');
    expect(document.activeElement).toBe(d.q('mcp-key-client'));
    expect(d.actions()).toEqual(['dismiss', 'create']);
    expect(d.action('create')!.disabled).toBe(true);
    await d.key('Enter');
    expect(d.memory.issued).toEqual([]);
    await d.type('   ');
    expect(d.action('create')!.disabled).toBe(true);
    await d.type('Pitwall');
    expect(d.action('create')!.disabled).toBe(false);
  });

  it('shows the key once, copies it, and answers true on Done', async () => {
    const d = await open();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    await d.type('Pitwall');
    await d.key('Enter');
    const shown = d.q('mcp-key-shown');
    expect(shown).not.toBeNull();
    const key = d.q<HTMLInputElement>('mcp-key-value')!.value;
    expect(key).toBe(d.memory.issued[0]);
    expect(text(shown!)).toContain('only time the key is shown');
    expect(d.q('mcp-key-form')).toBeNull();
    expect(d.actions()).toEqual(['copy', 'close', 'done']);
    await d.press('copy');
    expect(writeText).toHaveBeenCalledWith(key);
    expect(text(d.q('mcp-key-copy-status')!)).toContain('Copied');
    await d.press('done');
    expect(d.outcome()).toEqual({ kind: 'answered', value: true });
    expect(document.body.innerHTML).not.toContain(key);
  });

  it('says so when the clipboard is not available and still shows the key to select', async () => {
    const d = await open();
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('denied')) }, configurable: true });
    await d.type('x');
    await d.press('create');
    await d.press('copy');
    expect(text(d.q('mcp-key-copy-status')!)).toContain('clipboard is not available');
    expect(d.q<HTMLInputElement>('mcp-key-value')!.readOnly).toBe(true);
    d.q('mcp-key-value')!.dispatchEvent(new Event('focus'));
  });

  it('shows the engine’s refusal in the frame’s alert and Try again issues the key once it can', async () => {
    const d = await open({ issueError: { code: 'invalid', message: '16 keys are in use. Revoke one you no longer need first.', retryable: false } });
    await d.type('Pitwall');
    await d.press('create');
    expect(text(d.q('dialog-alert-text')!)).toContain('16 keys are in use');
    expect(document.activeElement).toBe(d.q('dialog-try-again'));
    expect(d.q('mcp-key-shown')).toBeNull();
    expect(d.q<HTMLInputElement>('mcp-key-client')!.value).toBe('Pitwall');
    expect(d.outcome()).toBeNull();
  });

  it('says Creating… on the busy Create key button while the engine issues the key', async () => {
    let release!: () => void;
    const gate = new Promise<void>((r) => (release = r));
    class Slow extends MemoryMcpBackend {
      override async issueKey(client: string) {
        await gate;
        return super.issueKey(client);
      }
    }
    const memory = new Slow();
    const d = await renderFramed(PdMcpKeyDialog, { id: 'pd.mcp.key', kind: 'form', modality: 'modal', providers: [provideMcpBackend({ backend: () => memory })] });
    const input = d.q<HTMLInputElement>('mcp-key-client')!;
    input.value = 'Pitwall';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await d.stable();
    expect(d.action('create')!.textContent!.trim()).toBe('Create key');
    void d.press('create');
    await d.stable();
    expect(d.action('create')!.textContent!.trim()).toBe('Creating…');
    release();
    await d.stable();
  });

  it('shows the key limit as a validation message on the name field and keeps Create key disabled', async () => {
    const memory = new MemoryMcpBackend();
    for (let i = 0; i < 16; i++) await memory.issueKey(`client ${i}`);
    const d = await renderFramed(PdMcpKeyDialog, { id: 'pd.mcp.key', kind: 'form', modality: 'modal', providers: [provideMcpBackend({ backend: () => memory })] });
    await d.stable();
    expect(text(d.q('mcp-key-limit')!)).toBe('16 keys are in use. Revoke one you no longer need first.');
    expect(d.q('mcp-key-client')!.getAttribute('aria-invalid')).toBe('true');
    const input = d.q<HTMLInputElement>('mcp-key-client')!;
    input.value = 'Pitwall';
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await d.stable();
    expect(d.action('create')!.disabled).toBe(true);
    expect(d.q('dialog-alert-text')).toBeNull();
  });

  it('uses curly quotes in its lede and the shared muted label style', async () => {
    const d = await open();
    expect(text(d.el)).toContain('“Pitwall”');
    expect(text(d.el)).not.toContain('"Pitwall"');
    const src = (await import('./mcp-key-dialog')).PdMcpKeyDialog as unknown as { ɵcmp: { styles: string[] } };
    const css = src.ɵcmp.styles.join('').replace(/\s+/g, ' ');
    expect(css).toMatch(/\.pd-key__field(?:\[_ngcontent-%COMP%\])? \{[^}]*color: var\(--(?:%NS%)?pd-text-muted\)[^}]*font-size: var\(--(?:%NS%)?pd-text-label\)[^}]*font-weight: var\(--(?:%NS%)?pd-weight-medium\)/);
  });

  it('is dismissed by Cancel and never issues a key', async () => {
    const d = await open();
    await d.press('dismiss');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
    expect(d.memory.issued).toEqual([]);
  });
});

describe('the revoke dialog', () => {
  const open = (inputs: { keyId?: number; client?: string } = {}) => renderFramed(PdMcpRevokeDialog, { id: 'pd.mcp.revoke', inputs });

  it('names the client in its question, makes Keep the focus and Enter target', async () => {
    const d = await open({ keyId: 3, client: 'Pitwall' });
    expect(d.heading()).toBe('Revoke the key of “Pitwall”?');
    expect(text(d.q('mcp-revoke-text')!)).toContain('stops working at once');
    expect(document.activeElement).toBe(d.action('keep'));
    expect(d.action('revoke')!.textContent!.trim()).toBe('Revoke key');

  });

  it('answers true on Revoke key and is dismissed by Keep', async () => {
    const a = await open({ keyId: 3, client: 'Pitwall' });
    await a.press('revoke');
    expect(a.outcome()).toEqual({ kind: 'answered', value: true });
    TestBed.resetTestingModule();
    const b = await open({ keyId: 3, client: 'Pitwall' });
    await b.press('keep');
    expect(b.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('says so, with a Close, when it was opened without a key', async () => {
    const d = await open();
    expect(text(d.q('mcp-revoke-invalid')!)).toContain('without a key');
    expect(d.actions()).toEqual(['close']);
    await d.press('close');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('names the key by its number when the client is unknown', async () => {
    const d = await open({ keyId: 9 });
    expect(d.heading()).toContain('“9”');
  });
});
