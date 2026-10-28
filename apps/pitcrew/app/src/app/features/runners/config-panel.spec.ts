import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { NOW, NoticeLog, appProviders, byText, flush, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';
import { ConfigPanel, parseCapacity, parseTriState, triState } from './config-panel';
import { PREVIEW_DEBOUNCE_MS } from './runner-config.store';

describe('config panel helpers', () => {
  it('reads the capacity box', () => {
    expect(parseCapacity('')).toBeNull();
    expect(parseCapacity('abc')).toBeNull();
    expect(parseCapacity('4')).toBe(4);
    expect(parseCapacity('2.9')).toBe(2);
    expect(parseCapacity('-3')).toBe(0);
  });
  it('maps tri-state selects', () => {
    expect(parseTriState('on')).toBe(true);
    expect(parseTriState('off')).toBe(false);
    expect(parseTriState('default')).toBeNull();
    expect([triState(null), triState(true), triState(false)]).toEqual(['default', 'on', 'off']);
  });
});

describe('ConfigPanel', () => {
  let mock: MockBackend;
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ['Date'] });
    vi.setSystemTime(NOW);
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock) });
  });
  afterEach(() => {
    vi.useRealTimers();
    document.querySelectorAll('.cdk-overlay-container > *').forEach((n) => n.remove());
    mock.dispose();
  });

  async function mount(name = 'ui-tests') {
    await mock.status();
    const f = TestBed.createComponent(ConfigPanel);
    f.componentRef.setInput('runner', name);
    await settle(f, 6);
    return { f, el: f.nativeElement as HTMLElement };
  }
  const log = () => TestBed.inject(NoticeLog);
  /** What the review window was last opened with, as one string (the diff itself is tested with the dialog). */
  const reviewed = () => JSON.stringify(log().dialogs.at(-1)?.inputs ?? null);
  const input = (el: HTMLElement, id: string) => el.querySelector(`#${id}`) as HTMLInputElement;
  async function edit(f: { detectChanges(): void }, field: HTMLInputElement | HTMLSelectElement | HTMLTextAreaElement, value: string, ev = 'input') {
    typeInto(field, value, ev);
    await flush(PREVIEW_DEBOUNCE_MS + 50);
    await settle(f as never, 3);
  }

  it('shows the file’s settings with the runner’s defaults as placeholders', async () => {
    const { el } = await mount();
    expect(text(el)).toContain('Runner configuration');
    expect(input(el, 'cfg-capacity').value).toBe('1');
    expect(input(el, 'cfg-capacity').placeholder).toBe('1');
    expect(input(el, 'cfg-timeout').value).toBe('3h');
    expect(input(el, 'cfg-timeout').placeholder).toBe('3h');
    expect(input(el, 'cfg-fetchInterval').placeholder).toBe('2s');
    expect(input(el, 'cfg-fetchTimeout').placeholder).toBe('5s');
    expect(text(el.querySelector('pc-string-list'))).toContain('windows:host');
    expect(text(el.querySelector('fieldset:nth-of-type(3)'))).toContain('NUGET_PACKAGES');
    expect(el.querySelector('[data-testid="containers"]')).toBeNull();
    expect(byText<HTMLButtonElement>(el, 'button', 'Review changes')!.disabled).toBe(true);
    expect(text(el.querySelector('[data-testid="label-check"]'))).toContain('Labels verified after restart: windows, webview2, desktop');
  });

  it('shows the Containers group for container runners and warns about privileged', async () => {
    const { f, el } = await mount('docker-lab');
    expect(el.querySelector('[data-testid="containers"]')).not.toBeNull();
    expect(text(el.querySelector('[data-testid="label-check"]'))).toContain('declared [linux-container, docker] but the file asks for');
    expect(text(el)).not.toContain('almost full access');
    await edit(f, input(el, 'cfg-privileged') as never, 'on', 'change');
    expect(text(el)).toContain('almost full access');
    await edit(f, input(el, 'cfg-containerNetwork'), 'lab-net');
    expect(el.querySelector('[data-testid="containers"] pc-string-list')).not.toBeNull();
  });

  it('asks the engine to check an edit and shows its message; errors disable review', async () => {
    const { f, el } = await mount();
    await edit(f, input(el, 'cfg-capacity'), '0');
    expect(text(el.querySelector('.field'))).toBeDefined();
    expect(text(el)).toContain('Error: Capacity must be between 1 and 64');
    expect(input(el, 'cfg-capacity').getAttribute('aria-invalid')).toBe('true');
    expect(byText<HTMLButtonElement>(el, 'button', 'Review changes')!.disabled).toBe(true);
    await edit(f, input(el, 'cfg-capacity'), '3');
    expect(text(el)).not.toContain('Capacity must be between');
    expect(byText<HTMLButtonElement>(el, 'button', 'Review changes')!.disabled).toBe(false);
    byText<HTMLButtonElement>(el, 'button', 'Discard changes')!.click();
    await settle(f, 2);
    expect(input(el, 'cfg-capacity').value).toBe('1');
  });

  it('edits labels, variables, cache and log level', async () => {
    const { f, el } = await mount();
    const labels = el.querySelector('#cfg-labels') as HTMLInputElement;
    typeInto(labels, 'gpu:host');
    labels.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await flush(PREVIEW_DEBOUNCE_MS + 50);
    await settle(f, 3);
    expect(text(el.querySelector('pc-string-list'))).toContain('gpu:host');
    (el.querySelector('button[aria-label="Remove windows:host"]') as HTMLElement).click();
    await settle(f, 2);
    typeInto(el.querySelector('input[aria-label="New variable name"]') as HTMLInputElement, 'FOO');
    typeInto(el.querySelector('input[aria-label="New variable value"]') as HTMLInputElement, 'bar');
    await settle(f, 1);
    (el.querySelectorAll('fieldset')[2]!.querySelector('button.btn:not(.icon)') as HTMLElement).click();
    await settle(f, 2);
    expect(text(el)).toContain('FOO');
    typeInto(el.querySelector('input[aria-label="Value of FOO"]') as HTMLInputElement, 'baz');
    (el.querySelector('button[aria-label="Remove NUGET_PACKAGES"]') as HTMLElement).click();
    await edit(f, input(el, 'cfg-cacheEnabled') as never, 'off', 'change');
    await edit(f, input(el, 'cfg-logLevel') as never, 'debug', 'change');
    await edit(f, input(el, 'cfg-cacheSizeLimit'), '10GB');
    await settle(f, 2);
    byText<HTMLButtonElement>(el, 'button', 'Review changes')!.click();
    await settle(f, 4);
    expect(log().dialogs.at(-1)?.id).toBe('pc.config-diff');
    const d = reviewed();
    for (const k of ['runner.labels', 'runner.envs', 'cache.enabled', 'log.level', 'cache.size_limit']) expect(d).toContain(k);
  });

  it('reviews the diff, then saves and shows the restart notice', async () => {
    const { f, el } = await mount('build-win');
    await edit(f, input(el, 'cfg-capacity'), '4');
    log().answerNext(true);
    byText<HTMLButtonElement>(el, 'button', 'Review changes')!.click();
    await settle(f, 4);
    expect(log().dialogs.at(-1)?.id).toBe('pc.config-diff');
    expect(reviewed()).toContain('capacity: 4');
    expect(reviewed()).toContain('"maxBackups":10');
    await settle(f, 8);
    expect(text(el.querySelector('[data-testid="saved-banner"]'))).toContain('Saved. The runner restarts after the current job.');
    expect(text(el.querySelector('[data-testid="restart-pending"]'))).toBe('Restarts after the current job');
    expect(input(el, 'cfg-capacity').value).toBe('4');
    byText<HTMLButtonElement>(el, 'button', 'Dismiss')!.click();
    await settle(f, 2);
    expect(el.querySelector('[data-testid="saved-banner"]')).toBeNull();
  });

  it('says only comments changed and needs no restart, and that a stopped runner starts later', async () => {
    const { f, el } = await mount('legacy-ci');
    byText<HTMLButtonElement>(el, 'button', 'Advanced (YAML)')!.click();
    await settle(f, 2);
    const ta = el.querySelector('textarea') as HTMLTextAreaElement;
    await edit(f, ta, `# note\n${ta.value}`);
    log().answerNext(true);
    byText<HTMLButtonElement>(el, 'button', 'Review changes')!.click();
    await settle(f, 4);
    expect(reviewed()).toContain('"restartNeeded":false');

    await settle(f, 8);
    expect(text(el.querySelector('[data-testid="saved-banner"]'))).toContain('Only comments or layout changed; no restart needed.');
    expect(text(el.querySelector('[data-testid="saved-banner"]'))).toContain('config.yaml.bak-');
    const { f: f2, el: el2 } = await mount('docker-lab');
    await edit(f2, input(el2, 'cfg-capacity'), '2');
    log().answerNext(true);
    byText<HTMLButtonElement>(el2, 'button', 'Review changes')!.click();
    await settle(f2, 4);
    await settle(f2, 8);
    expect(text(el2.querySelector('[data-testid="saved-banner"]'))).toContain('used the next time the runner starts');
  });

  it('Advanced shows line numbers, the engine’s findings with lines, and jumps to a line', async () => {
    const { f, el } = await mount();
    byText<HTMLButtonElement>(el, 'button', 'Advanced (YAML)')!.click();
    await settle(f, 2);
    expect(text(el.querySelector('[data-testid="yaml-status"]'))).toBe('Valid YAML');
    const ta = el.querySelector('textarea') as HTMLTextAreaElement;
    expect(ta.getAttribute('aria-label')).toBe('config.yaml');
    expect(el.querySelectorAll('.gutter span').length).toBe(ta.value.split('\n').length);
    const lines = ta.value.split('\n');
    const at = lines.findIndex((l) => l.includes('capacity:'));
    lines[at] = '\t  capacity: 1';
    await edit(f, ta, lines.join('\n'));
    expect(text(el.querySelector('[data-testid="yaml-status"]'))).toBe('1 error');
    expect(text(el.querySelector('.findings'))).toContain(`Error on line ${at + 1}:`);
    expect(el.querySelector('.gutter .err')!.textContent).toContain(String(at + 1));
    expect(text(el.querySelector('[data-testid="raw-note"]'))).toContain('You edited the file directly; save or discard to use the form again.');
    (el.querySelector('.finding') as HTMLElement).click();
    expect(ta.selectionStart).toBe(lines.slice(0, at).reduce((n, l) => n + l.length + 1, 0));
    expect(byText<HTMLButtonElement>(el, 'button', 'Review changes')!.disabled).toBe(true);
    expect((byText<HTMLButtonElement>(el, 'button', /^Form$/))!.disabled).toBe(true);
    byText<HTMLButtonElement>(el, 'button', 'Discard changes')!.click();
    await settle(f, 2);
    expect(el.querySelector('textarea')).toBeNull();
    expect(input(el, 'cfg-capacity').value).toBe('1');
  });

  it('warnings are shown but allow saving', async () => {
    const { f, el } = await mount();
    byText<HTMLButtonElement>(el, 'button', 'Advanced (YAML)')!.click();
    await settle(f, 2);
    const ta = el.querySelector('textarea') as HTMLTextAreaElement;
    await edit(f, ta, `${ta.value}\nsurprise: 1\n`);
    expect(text(el.querySelector('[data-testid="yaml-status"]'))).toBe('Valid YAML, 1 warning');
    expect(byText<HTMLButtonElement>(el, 'button', 'Review changes')!.disabled).toBe(false);
    byText<HTMLButtonElement>(el, 'button', 'Review changes')!.click();
    await settle(f, 4);
    expect(reviewed()).toContain('surprise');
    await settle(f, 2);
    byText<HTMLButtonElement>(el, 'button', 'Discard changes')!.click();
    await settle(f, 2);
    byText<HTMLButtonElement>(el, 'button', 'Advanced (YAML)')!.click();
    await settle(f, 2);
    byText<HTMLButtonElement>(el, 'button', /^Form$/)!.click();
    await settle(f, 2);
    expect(el.querySelector('textarea')).toBeNull();
  });

  it('shows a conflict with Reload when the file changed on disk', async () => {
    const { f, el } = await mount();
    await edit(f, input(el, 'cfg-capacity'), '2');
    const other = await mock.runnerConfig('ui-tests');
    await mock.saveRunnerConfig('ui-tests', { mode: 'raw', text: `# other\n${other.text}` }, other.fingerprint);
    log().answerNext(true);
    byText<HTMLButtonElement>(el, 'button', 'Review changes')!.click();
    await settle(f, 6);
    expect(text(el.querySelector('[data-testid="conflict"]'))).toContain('The file changed on disk');
    byText<HTMLButtonElement>(el.querySelector('[data-testid="conflict"]') as HTMLElement, 'button', 'Reload')!.click();
    await settle(f, 6);
    expect(el.querySelector('[data-testid="conflict"]')).toBeNull();
  });

  it('shows other save errors', async () => {
    const { f, el } = await mount();
    await edit(f, input(el, 'cfg-capacity'), '2');
    vi.spyOn(mock, 'saveRunnerConfig').mockRejectedValueOnce({ code: 'permissionDenied', message: 'config.yaml is read-only' });
    log().answerNext(true);
    byText<HTMLButtonElement>(el, 'button', 'Review changes')!.click();
    await settle(f, 6);
    expect(text(el.querySelector('[role="alert"]'))).toContain('config.yaml is read-only');
  });

  it('notes keys the form cannot show, a missing file, and an engine check that failed', async () => {
    const real = mock.runnerConfig.bind(mock);
    vi.spyOn(mock, 'runnerConfig').mockImplementation(async (r) => {
      const c = await real(r);
      return { ...c, text: '', view: { ...c.view, unreadable: ['runner.insecure', 'host.workdir_parent'] } };
    });
    const { f, el } = await mount();
    expect(text(el)).toContain('Some keys are kept as they are');
    expect(text(el)).toContain('runner.insecure, host.workdir_parent');
    expect(text(el)).toContain('does not exist yet');
    vi.spyOn(mock, 'previewRunnerConfig').mockRejectedValueOnce({ code: 'timeout', message: 'no answer' });
    await edit(f, input(el, 'cfg-capacity'), '2');
    expect(text(el)).toContain('The change could not be checked');
  });

  it('lists findings about the file as a whole and a restart waiting in the status', async () => {
    const real = mock.runnerConfig.bind(mock);
    vi.spyOn(mock, 'runnerConfig').mockImplementation(async (r) => {
      const c = await real(r);
      return { ...c, restartPending: true, view: { ...c.view, issues: [{ severity: 'warning', path: 'surprise', line: 4, message: 'Unknown key "surprise".' }] } };
    });
    const { el } = await mount();
    expect(text(el)).toContain('Unknown key "surprise".');
    expect(el.querySelector('[data-testid="restart-pending"]')).not.toBeNull();
  });

  it('shows a load error with Retry', async () => {
    vi.spyOn(mock, 'runnerConfig').mockRejectedValueOnce({ code: 'internal', message: 'cannot read config.yaml' });
    const { f, el } = await mount();
    expect(text(el)).toContain('could not be loaded');
    byText<HTMLButtonElement>(el, 'button', 'Retry')!.click();
    await settle(f, 6);
    expect(input(el, 'cfg-capacity')).not.toBeNull();
  });

  it('refreshes the label check when the runner restarts', async () => {
    const { f } = await mount('ui-tests');
    const spy = vi.spyOn(mock, 'activity');
    await mock.restart('ui-tests');
    await flush(1800);
    await settle(f, 4);
    expect(spy).toHaveBeenCalled();
  });
});
