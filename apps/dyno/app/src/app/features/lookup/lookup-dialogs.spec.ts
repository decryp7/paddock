import { DIALOG_DATA, DialogRef } from '@angular/cdk/dialog';
import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { Dialogs } from '../../shared/dialog';
import { provideIcons } from '../../shared/icons';
import { HashSearchDialog } from './hash-search-dialog';
import { Lookups } from './lookups';
import { WebSearchDialog } from './web-search-dialog';

function setup<T>(component: new () => T, data: unknown, backend = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 })) {
  const close = vi.fn();
  TestBed.configureTestingModule({
    providers: [provideIcons(), { provide: BACKEND, useValue: backend }, { provide: DIALOG_DATA, useValue: data }, { provide: DialogRef, useValue: { close } }],
  });
  const f = TestBed.createComponent(component);
  f.detectChanges();
  return { f, close, el: f.nativeElement as HTMLElement, backend };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 5; i++) {
    await f.whenStable();
    f.detectChanges();
  }
};
const btn = (el: HTMLElement, label: string) => [...el.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.trim().includes(label))!;
const field = (el: HTMLElement) => el.querySelector<HTMLInputElement>('input[name=query]')!;

describe('WebSearchDialog', () => {
  it('shows the redacted text, says what was removed, and searches for exactly the edited text', async () => {
    const { f, el, close, backend } = setup(WebSearchDialog, { text: 'Slack crashed for alex at 10.0.0.5', subject: 'Slack' });
    expect(el.textContent).toContain('Nothing leaves your computer until you click Search');
    await settle(f);
    expect(field(el).value).toBe('Slack crashed for at');
    expect(el.textContent).toContain('Removed from the text: 1 user name, 1 IP address.');
    field(el).value = 'slack crashed on startup';
    field(el).dispatchEvent(new Event('input'));
    await settle(f);
    btn(el, 'Search').click();
    await settle(f);
    expect((backend as MockBackend).opened).toEqual(['https://duckduckgo.com/?q=slack+crashed+on+startup']);
    expect(close).toHaveBeenCalledWith(true);
  });

  it('searches nothing before the click and Search is off for an empty text', async () => {
    const { f, el, backend, close } = setup(WebSearchDialog, { text: 'kernel_task', subject: 'kernel_task' });
    await settle(f);
    expect((backend as MockBackend).opened).toEqual([]);
    expect(el.textContent).toContain('Nothing private was found in this text.');
    field(el).value = '   ';
    field(el).dispatchEvent(new Event('input'));
    await settle(f);
    expect(btn(el, 'Search').disabled).toBe(true);
    btn(el, 'Cancel').click();
    expect(close).toHaveBeenCalledWith();
  });

  it('enter in the field searches', async () => {
    const { f, el, close } = setup(WebSearchDialog, { text: 'Figma', subject: 'Figma' });
    await settle(f);
    field(el).dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    await settle(f);
    expect(close).toHaveBeenCalledWith(true);
  });

  it('a failing search stays open with the reason', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, fail: new Set(['webSearch']) });
    const { f, el, close } = setup(WebSearchDialog, { text: 'Figma', subject: 'Figma' }, mock);
    await settle(f);
    btn(el, 'Search').click();
    await settle(f);
    expect(el.querySelector('[role=alert]')).not.toBeNull();
    expect(close).not.toHaveBeenCalled();
  });

  it('a failing text preparation shows an error and no search field', async () => {
    const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000, fail: new Set(['searchSeed']) });
    const { f, el } = setup(WebSearchDialog, { text: 'x', subject: 'x' }, mock);
    await settle(f);
    expect(field(el)).toBeNull();
    expect(el.querySelector('[role=alert]')).not.toBeNull();
    expect(btn(el, 'Search').disabled).toBe(true);
  });
});

describe('HashSearchDialog', () => {
  const data = { source: { path: '/Applications/Slack.app/Contents/MacOS/Slack' }, name: 'Slack' };

  it('shows the hash before anything opens, then opens VirusTotal with only the hash', async () => {
    const { f, el, close, backend } = setup(HashSearchDialog, data);
    await settle(f);
    const hash = el.querySelector('code')!.textContent!;
    expect(hash).toMatch(/^[0-9a-f]{64}$/);
    expect(el.textContent).toContain('Nothing is uploaded');
    expect(el.textContent).toContain('/Applications/Slack.app/Contents/MacOS/Slack');
    expect((backend as MockBackend).opened).toEqual([]);
    btn(el, 'Open VirusTotal').click();
    await settle(f);
    expect((backend as MockBackend).opened).toEqual([`https://www.virustotal.com/gui/file/${hash}`]);
    expect(close).toHaveBeenCalledWith(true);
  });

  it('copies the hash', async () => {
    const write = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: write }, configurable: true });
    const { f, el } = setup(HashSearchDialog, data);
    await settle(f);
    el.querySelector<HTMLButtonElement>('button[aria-label="Copy the hash"]')!.click();
    await settle(f);
    expect(write).toHaveBeenCalledWith(expect.stringMatching(/^[0-9a-f]{64}$/));
  });

  it('an unreadable file shows the reason with Try again, and Open is off', async () => {
    const { f, el } = setup(HashSearchDialog, { source: { path: 'relative' }, name: 'x' });
    await settle(f);
    expect(el.querySelector('[role=alert]')?.textContent).toContain('full path');
    expect(btn(el, 'Open VirusTotal').disabled).toBe(true);
    expect(btn(el, 'Try again')).toBeTruthy();
    btn(el, 'Try again').click();
    await settle(f);
    expect(el.querySelector('[role=alert]')).not.toBeNull();
  });
});

describe('Lookups', () => {
  it('opens each dialog and resolves false when it is closed without opening the browser', async () => {
    const backend = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
    TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: backend }] });
    const dialogs = TestBed.inject(Dialogs);
    const open = vi.spyOn(dialogs, 'open').mockReturnValue({ closed: { subscribe: (o: { next: (v: unknown) => void; complete: () => void }) => { o.next(undefined); o.complete(); return { unsubscribe() {} }; } } } as never);
    const lookups = TestBed.inject(Lookups);
    expect(await lookups.searchWeb('Slack', 'Slack')).toBe(false);
    expect(await lookups.searchHash({ path: '/a' }, 'Slack')).toBe(false);
    expect(open).toHaveBeenCalledTimes(2);
    expect(open.mock.calls[0]![1]).toEqual({ text: 'Slack', subject: 'Slack' });
    expect(open.mock.calls[1]![1]).toEqual({ source: { path: '/a' }, name: 'Slack' });
  });
});
