import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { MAX_KEYS, MCP_KEY_DIALOG_ID, MCP_REVOKE_DIALOG_ID, MemoryMcpBackend, provideShellMcp, type MemoryMcpOptions } from '@decryp7/paddock-ui/shell/mcp';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { PdMcpPage } from './mcp-page';

async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

/** The frame in which pd-table (Material's table in the CDK viewport) draws the rows of its range. */
const nextFrame = (): Promise<void> => new Promise((done) => requestAnimationFrame(() => done()));

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

/** The dialog service as the page uses it: records what was opened and answers like the dialogs (the key dialog's window issues the key). */
class FakeDialogs {
  readonly opened: { id: string; inputs: Record<string, unknown> }[] = [];
  /** What the revoke confirmation answers: true is Revoke. */
  revokeAnswer: boolean | undefined = true;
  constructor(private readonly memory: MemoryMcpBackend) {}
  open(id: string, options: { inputs?: Record<string, unknown> } = {}) {
    this.opened.push({ id, inputs: options.inputs ?? {} });
    const closed = (async () => {
      if (id === MCP_REVOKE_DIALOG_ID) return this.revokeAnswer;
      await this.memory.issueKey('Pitwall');
      return true;
    })();
    return { closed, close: () => undefined };
  }
}

describe('the AI access page', () => {
  async function open(options: MemoryMcpOptions = {}) {
    const memory = new MemoryMcpBackend(options);
    const dialogs = new FakeDialogs(memory);
    TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), provideShellMcp({ backend: () => memory }), { provide: DialogService, useValue: dialogs }] });
    const fixture = TestBed.createComponent(PdMcpPage);
    const el = fixture.nativeElement as HTMLElement;
    const flush = async (): Promise<void> => {
      await settle();
      await fixture.whenStable();
      await settle();
      await fixture.whenStable();
      await nextFrame();
      await fixture.whenStable();
    };
    await flush();
    const q = (id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);
    const click = async (id: string | HTMLElement | null): Promise<void> => {
      (typeof id === 'string' ? q(id) : id)!.click();
      await flush();
    };
    const toggle = async (id: string): Promise<void> => click(id);
    return { fixture, el, q, click, toggle, flush, memory, dialogs };
  }

  it('shows it off, the read-only tools, the action apart and off, no clients yet, and an empty log', async () => {
    const p = await open();
    expect(text(p.q('mcp-page')!)).toContain('AI access (MCP)');
    expect(text(p.el)).toContain('AI access is off');
    expect((p.q('mcp-enabled') as HTMLInputElement).checked).toBe(false);
    expect((p.q('mcp-tool-search_events') as HTMLInputElement).checked).toBe(true);
    expect(text(p.q('mcp-tools')!)).toContain('Search events');
    expect(text(p.q('mcp-tools')!)).not.toContain('Export a report');
    expect(text(p.q('mcp-actions')!)).toContain('Allow Export a report');
    expect(text(p.q('mcp-actions-warning')!)).toContain('change something');
    expect((p.q('mcp-action-export_report') as HTMLInputElement).checked).toBe(false);
    expect(p.q('mcp-no-clients')).not.toBeNull();
    expect(p.q('mcp-keys')).toBeNull();
    expect(text(p.el)).toContain('No client has called a tool yet.');
  });

  it('turns it on and off, saving each change', async () => {
    const p = await open();
    await p.toggle('mcp-enabled');
    expect((await p.memory.get()).settings.enabled).toBe(true);
    expect(text(p.el)).toContain('AI access is on');
    await p.toggle('mcp-tool-errors_around');
    expect((await p.memory.get()).settings.disabledTools).toEqual(['errors_around']);
    await p.toggle('mcp-action-export_report');
    expect((await p.memory.get()).settings.allowedActions).toEqual(['export_report']);
    await p.toggle('mcp-resources');
    expect((await p.memory.get()).settings.resources).toBe(false);
    await p.toggle('mcp-enabled');
    expect(text(p.el)).toContain('AI access is off');
  });

  it('says why a change was refused, in the engine’s words, and the switch stays where the engine has it', async () => {
    const p = await open({ saveError: { code: 'storeUnavailable', message: 'The settings file is read-only.', retryable: true } });
    await p.toggle('mcp-enabled');
    expect(text(p.q('mcp-save-error')!)).toContain('The settings file is read-only.');
    expect((p.q('mcp-enabled') as HTMLInputElement).checked).toBe(false);
    await p.click('mcp-save-error-dismiss');
    expect(p.q('mcp-save-error')).toBeNull();
  });

  it('adds a client in the key dialog’s window and lists it afterwards, and the page never sees the key', async () => {
    const p = await open();
    await p.click('mcp-add-client');
    expect(p.dialogs.opened).toEqual([{ id: MCP_KEY_DIALOG_ID, inputs: {} }]);
    expect(text(p.q('mcp-keys')!)).toContain('Pitwall');
    expect(text(p.q('mcp-keys')!)).toContain('Active');
    for (const key of p.memory.issued) expect(p.fixture.nativeElement.innerHTML).not.toContain(key);
    expect(p.q('mcp-no-clients')).toBeNull();
  });

  it('revokes a client after the confirmation, and keeps it when the user keeps it', async () => {
    const p = await open();
    await p.click('mcp-add-client');
    p.dialogs.revokeAnswer = false;
    await p.click(p.q('mcp-revoke'));
    expect(p.dialogs.opened.at(-1)).toMatchObject({ id: MCP_REVOKE_DIALOG_ID, inputs: { keyId: 1, client: 'Pitwall' } });
    expect(text(p.q('mcp-keys')!)).toContain('Active');
    p.dialogs.revokeAnswer = undefined;
    await p.click(p.q('mcp-revoke'));
    expect(text(p.q('mcp-keys')!)).toContain('Active');
    p.dialogs.revokeAnswer = true;
    await p.click(p.q('mcp-revoke'));
    expect(text(p.q('mcp-keys')!)).toContain('Revoked');
    expect(p.q('mcp-revoke')).toBeNull();
  });

  it('shows the engine’s refusal when a key cannot be revoked', async () => {
    const p = await open({ revokeError: { code: 'storeUnavailable', message: 'The credential store is locked.', retryable: true } });
    await p.click('mcp-add-client');
    await p.click(p.q('mcp-revoke'));
    expect(text(p.q('mcp-save-error')!)).toContain('The credential store is locked.');
    expect(text(p.q('mcp-keys')!)).toContain('Active');
  });

  it('stops adding clients at the limit and says why', async () => {
    const p = await open();
    for (let i = 0; i < MAX_KEYS; i++) await p.memory.issueKey(`c${i}`);
    await p.fixture.componentInstance['store'].refresh();
    await p.flush();
    expect(p.q('mcp-add-client')!.getAttribute('aria-disabled')).toBe('true');
    expect(text(p.q('mcp-keys-limit')!)).toContain(`${MAX_KEYS} keys are in use`);
    const before = p.dialogs.opened.length;
    await p.click('mcp-add-client');
    expect(p.dialogs.opened).toHaveLength(before);
  });

  it('shows the activity log as it grows, redacted arguments and all, and clears it', async () => {
    const p = await open();
    p.memory.recordCall({ client: 'Pitwall', tool: 'search_events', arguments: { query: '<omitted: 5 characters>', limit: 5 }, resultBytes: 2048, durationMs: 12, outcome: 'ok' });
    p.memory.recordCall({ client: 'Pitwall', tool: 'export_report', arguments: {}, resultBytes: 0, durationMs: 1, outcome: 'refused' });
    await p.flush();
    const log = text(p.q('mcp-audit')!);
    expect(log).toContain('search_events');
    expect(log).toContain('export_report');
    expect(log).toContain('Pitwall');
    expect(text(p.el)).not.toContain('No client has called a tool yet.');
    await p.click('mcp-audit-clear');
    expect(text(p.el)).toContain('No client has called a tool yet.');
    expect((p.q('mcp-audit-clear') as HTMLButtonElement).disabled).toBe(true);
  });

  it('shows a failed log read inside the log with Retry, and the rest of the page stays', async () => {
    const p = await open({ auditFailures: 1 });
    expect(p.q('mcp-enabled')).not.toBeNull();
    expect(text(p.q('mcp-audit')!)).toContain('could not be read');
    const retry = [...p.el.querySelectorAll('pd-mcp-audit button')].find((b) => text(b) === 'Retry') as HTMLElement;
    retry.click();
    await p.flush();
    expect(text(p.el)).toContain('No client has called a tool yet.');
  });

  it('shows a failed first ask as an inline error with Retry', async () => {
    const p = await open({ getFailures: 1 });
    expect(p.q('mcp-error')).not.toBeNull();
    expect(text(p.q('mcp-error')!)).toContain('could not be read');
    expect(p.q('mcp-enabled')).toBeNull();
    const retry = [...p.q('mcp-error')!.querySelectorAll('button')].find((b) => text(b) === 'Retry') as HTMLElement;
    retry.click();
    await p.flush();
    expect(p.q('mcp-error')).toBeNull();
    expect(p.q('mcp-enabled')).not.toBeNull();
  });
});
