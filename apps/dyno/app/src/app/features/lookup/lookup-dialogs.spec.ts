import { TestBed } from '@angular/core/testing';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { provideIcons } from '../../shared/icons';
import { HashSearchDialog } from './hash-search-dialog';
import { Lookups } from './lookups';
import { WebSearchDialog } from './web-search-dialog';

const mock = (fail?: string) => new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, ...(fail ? { fail: new Set([fail]) } : {}) });
const open = (component: Parameters<typeof renderFramed>[0], inputs: Record<string, unknown>, backend: MockBackend, kind: 'form' | 'viewer') =>
  renderFramed(component, { id: 'dy.test', kind, modality: kind === 'form' ? 'modal' : 'modeless', inputs, providers: [provideIcons(), { provide: BACKEND, useValue: backend }] });
const settle = async (d: { stable(): Promise<void> }) => {
  for (let i = 0; i < 4; i++) {
    await d.stable();
    await new Promise((r) => setTimeout(r));
  }
};
const field = (d: { el: HTMLElement }) => d.el.querySelector<HTMLInputElement>('input[name=query]')!;

describe('WebSearchDialog', () => {
  it('shows the redacted text, says what was removed, and searches for exactly the edited text', async () => {
    const backend = mock();
    const d = await open(WebSearchDialog, { text: 'Slack crashed for alex at 10.0.0.5', subject: 'Slack' }, backend, 'form');
    expect(d.el.textContent).toContain('Nothing leaves your computer until you click Search');
    await settle(d);
    expect(field(d).value).toBe('Slack crashed for at');
    expect(d.el.textContent).toContain('Removed from the text: 1 user name, 1 IP address.');
    field(d).value = 'slack crashed on startup';
    field(d).dispatchEvent(new Event('input'));
    await settle(d);
    await d.press('search');
    await settle(d);
    expect(backend.opened).toEqual(['https://duckduckgo.com/?q=slack+crashed+on+startup']);
    expect(d.outcome()).toEqual({ kind: 'answered', value: true });
  });

  it('searches nothing before the click, Search is off for an empty text, Cancel dismisses', async () => {
    const backend = mock();
    const d = await open(WebSearchDialog, { text: 'kernel_task', subject: 'kernel_task' }, backend, 'form');
    await settle(d);
    expect(backend.opened).toEqual([]);
    expect(d.el.textContent).toContain('Nothing private was found in this text.');
    field(d).value = '   ';
    field(d).dispatchEvent(new Event('input'));
    await settle(d);
    expect(d.action('search')!.disabled).toBe(true);
    await d.press('cancel');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
  });

  it('a failing search stays open and the frame says why, with Try again', async () => {
    const d = await open(WebSearchDialog, { text: 'Figma', subject: 'Figma' }, mock('webSearch'), 'form');
    await settle(d);
    await d.press('search');
    await settle(d);
    expect(d.el.querySelector('[role=alert]')).not.toBeNull();
    expect(d.outcome()).toBeNull();
  });

  it('a failing text preparation shows an error and no search field', async () => {
    const d = await open(WebSearchDialog, { text: 'x', subject: 'x' }, mock('searchSeed'), 'form');
    await settle(d);
    expect(field(d)).toBeNull();
    expect(d.el.querySelector('[role=alert]')).not.toBeNull();
    expect(d.action('search')!.disabled).toBe(true);
  });
});

describe('HashSearchDialog', () => {
  const inputs = { source: { path: '/Applications/Slack.app/Contents/MacOS/Slack' }, name: 'Slack' };

  it('shows the hash before anything opens, then opens VirusTotal with only the hash', async () => {
    const backend = mock();
    const d = await open(HashSearchDialog, inputs, backend, 'viewer');
    await settle(d);
    const hash = d.el.querySelector('code')!.textContent!;
    expect(hash).toMatch(/^[0-9a-f]{64}$/);
    expect(d.el.textContent).toContain('Nothing is uploaded');
    expect(d.el.textContent).toContain('/Applications/Slack.app/Contents/MacOS/Slack');
    expect(backend.opened).toEqual([]);
    await d.press('open');
    await settle(d);
    expect(backend.opened).toEqual([`https://www.virustotal.com/gui/file/${hash}`]);
    expect(d.outcome()).toEqual({ kind: 'answered', value: true });
  });

  it('copies the hash with a footer action', async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    const d = await open(HashSearchDialog, inputs, mock(), 'viewer');
    await settle(d);
    await d.press('copy');
    await settle(d);
    expect(write).toHaveBeenCalledWith(expect.stringMatching(/^[0-9a-f]{64}$/));
    expect(d.outcome()).toBeNull();
  });

  it('an unreadable file shows the reason; pressing Open VirusTotal reads again and the frame offers Try again', async () => {
    const d = await open(HashSearchDialog, { source: { path: 'relative' }, name: 'x' }, mock(), 'viewer');
    await settle(d);
    expect(d.el.querySelector('[role=alert]')?.textContent).toContain('full path');
    expect(d.action('copy')!.disabled).toBe(true);
    await d.press('open');
    await settle(d);
    expect(d.el.querySelector('[role=alert]')).not.toBeNull();
    expect(d.outcome()).toBeNull();
  });
});

describe('Lookups', () => {
  it('opens each dialog by id with its input, and resolves false when it is dismissed', async () => {
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: mock() }] });
    const dialogs = TestBed.inject(DialogService);
    const opened = vi.spyOn(dialogs, 'open').mockReturnValue({ closed: Promise.resolve({ kind: 'dismissed' }), dismiss() {} } as never);
    const lookups = TestBed.inject(Lookups);
    expect(await lookups.searchWeb('Slack', 'Slack')).toBe(false);
    expect(await lookups.searchHash({ path: '/a' }, 'Slack')).toBe(false);
    expect(opened.mock.calls[0]![0]).toBe('dy.web-search');
    expect(opened.mock.calls[0]![1]).toMatchObject({ inputs: { text: 'Slack', subject: 'Slack' } });
    expect(opened.mock.calls[1]![0]).toBe('dy.file-hash');
    expect(opened.mock.calls[1]![1]).toMatchObject({ inputs: { source: { path: '/a' }, name: 'Slack' } });
  });
});
