import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { HelperCard } from './helper-card';

function render(prepare: (mock: MockBackend) => void = () => undefined) {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  prepare(mock);
  TestBed.configureTestingModule({ providers: [{ provide: BACKEND, useValue: mock }] });
  const f = TestBed.createComponent(HelperCard);
  f.detectChanges();
  return { f, el: f.nativeElement as HTMLElement, mock };
}
const settle = async (f: { detectChanges(): void; whenStable(): Promise<unknown> }) => {
  for (let i = 0; i < 6; i++) {
    await f.whenStable();
    f.detectChanges();
    await new Promise((r) => setTimeout(r, 0));
  }
};
const text = (el: HTMLElement) => el.textContent?.replace(/\s+/g, ' ') ?? '';
const clickIn = (root: ParentNode, label: string) => [...root.querySelectorAll<HTMLButtonElement>('button')].find((b) => b.textContent?.includes(label))!.click();

describe('dy-helper-card', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = '')));

  it('says it is not installed and offers to install it, never installing by itself', async () => {
    const { f, el, mock } = render();
    await settle(f);
    expect(text(el)).toContain('Not installed');
    expect(text(el)).toContain('Dyno never runs as administrator');
    expect(el.querySelector('button.primary')?.textContent).toContain('Install the helper');
    expect(el.textContent).not.toContain('Remove the helper');
    expect((await mock.helperStatus()).state).toBe('notInstalled');
  });

  it('install goes through the explanation dialog, then shows Ready with a Remove button', async () => {
    const { f, el } = render();
    await settle(f);
    clickIn(el, 'Install the helper');
    await settle(f);
    const dialog = document.querySelector('dy-helper-dialog') as HTMLElement;
    expect(dialog.textContent).toContain('asks you to confirm');
    clickIn(dialog, 'Install the helper');
    await settle(f);
    expect(document.querySelector('dy-helper-dialog')).toBeNull();
    expect(text(el)).toContain('Ready');
    expect(text(el)).toContain('Version 0.1.0');
    expect(text(el)).toContain('Remove the helper');
  });

  it('removing asks first and then says not installed again', async () => {
    const { f, el, mock } = render();
    await mock.installHelper();
    await settle(f);
    // The card loads on init; reload after the install above.
    f.componentInstance.ngOnInit();
    await settle(f);
    clickIn(el, 'Remove the helper');
    await settle(f);
    const confirm = document.querySelector('dy-confirm-dialog') as HTMLElement;
    expect(confirm.textContent).toContain('Changes it made stay in force');
    clickIn(confirm, 'Remove the helper');
    await settle(f);
    expect(text(el)).toContain('Not installed');
  });

  it('shows what the helper did lately, with the reason when it refused', async () => {
    const { f, el } = render((mock) => vi.spyOn(mock, 'helperStatus').mockResolvedValue({
      state: 'ready',
      message: 'The helper is installed and ready.',
      version: '1.0.0',
      prompt: 'p',
      error: null,
      recent: [
        { at: 1_700_000_000, peer: 4, what: 'Turn hibernation off', verdict: 'applied', before: 'on', after: 'off', detail: '' },
        { at: 1_699_999_000, peer: 4, what: 'A connection was refused', verdict: 'refused', before: null, after: null, detail: 'process 9 is not the Dyno agent' },
      ],
    }));
    await settle(f);
    const rows = [...el.querySelectorAll('table tbody tr')].map((r) => text(r as HTMLElement));
    expect(rows[0]).toContain('Turn hibernation off');
    expect(rows[0]).toContain('Done');
    expect(rows[1]).toContain('not the Dyno agent');
    expect(rows[1]).toContain('Refused');
  });

  it('an outdated or silent helper offers to update or repair it, and an unavailable one explains why', async () => {
    for (const [state, label] of [
      ['outdated', 'Update the helper'],
      ['unreachable', 'Repair the helper'],
    ] as const) {
      TestBed.resetTestingModule();
      const { f, el } = render((mock) => vi.spyOn(mock, 'helperStatus').mockResolvedValue({ state, message: 'm', version: null, prompt: 'p', recent: [], error: 'Last time it failed.' }));
      await settle(f);
      expect(el.querySelector('button.primary')?.textContent).toContain(label);
      expect(text(el)).toContain('Last time it failed.');
    }
    TestBed.resetTestingModule();
    const { f, el } = render((mock) => vi.spyOn(mock, 'helperStatus').mockResolvedValue({ state: 'unsupported', message: 'pkexec is not installed.', version: null, prompt: 'p', recent: [], error: null }));
    await settle(f);
    expect(text(el)).toContain('pkexec is not installed.');
    expect(el.querySelector('button.primary')).toBeNull();
  });

  it('a failed status call shows an error with Retry', async () => {
    const { f, el } = render((mock) => vi.spyOn(mock, 'helperStatus').mockRejectedValue({ code: 'agentUnavailable', message: 'The agent is not running.' }));
    await settle(f);
    expect(el.querySelector('[role="alert"]')?.textContent).toContain('The agent is not running.');
  });
});
