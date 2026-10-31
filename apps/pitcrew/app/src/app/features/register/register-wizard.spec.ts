import { DIALOG_REF } from '@decryp7/paddock-ui/shell/dialogs';
import { TestBed } from '@angular/core/testing';
import { MockBackend } from '../../core/mock-backend';
import { NoticeLog } from '../../../testing/test-helpers';
import { appProviders, flush, makeMock, settle, text, type as typeInto } from '../../../testing/test-helpers';
import { RegisterWizard, stepErrors, toRequest, type RegisterDraft } from './register-wizard';

const draft = (over: Partial<RegisterDraft> = {}): RegisterDraft => ({
  serverUrl: 'https://git.example.com',
  token: 'abcdefgh',
  rememberToken: false,
  runner: 'build-win',
  displayName: '',
  labels: [],
  folder: 'default',
  dir: '',
  session: 'interactive',
  loggedOff: 'wait',
  ...over,
});

describe('stepErrors', () => {
  it('validates the server step', () => {
    expect(stepErrors(0, draft())).toEqual([]);
    expect(stepErrors(0, draft({ serverUrl: 'git.example.com', token: '  ' }))).toHaveLength(2);
    expect(stepErrors(0, draft({ serverUrl: 'https://' }))).toHaveLength(1);
    expect(stepErrors(0, draft({ serverUrl: 'http://host:3000/x' }))).toEqual([]);
  });
  it('validates the name step', () => {
    expect(stepErrors(1, draft())).toEqual([]);
    for (const bad of ['', 'Upper', '-x', 'a b', 'a'.repeat(33)]) expect(stepErrors(1, draft({ runner: bad }))).toHaveLength(1);
    expect(stepErrors(1, draft({ runner: 'a'.repeat(32) }))).toEqual([]);
  });
  it('validates the folder step and leaves the others alone', () => {
    expect(stepErrors(2, draft({ folder: 'existing', dir: ' ' }))).toHaveLength(1);
    expect(stepErrors(2, draft({ folder: 'existing', dir: 'C:\\r' }))).toEqual([]);
    expect(stepErrors(2, draft())).toEqual([]);
    expect(stepErrors(3, draft({ serverUrl: '' }))).toEqual([]);
  });
});

describe('toRequest', () => {
  it('trims, strips trailing slashes and applies defaults', () => {
    expect(toRequest(draft({ serverUrl: ' https://git.example.com// ', token: ' tok ', labels: ['a'], rememberToken: true }))).toEqual({
      runner: 'build-win',
      serverUrl: 'https://git.example.com',
      token: 'tok',
      displayName: null,
      labels: ['a'],
      dir: null,
      session: 'interactive',
      loggedOff: 'wait',
      rememberToken: true,
    });
  });
  it('adopts an existing folder and forces wait for the service session', () => {
    const r = toRequest(draft({ folder: 'existing', dir: ' C:\\r ', displayName: ' Name ', session: 'service', loggedOff: 'service-account' }));
    expect(r.dir).toBe('C:\\r');
    expect(r.displayName).toBe('Name');
    expect(r.loggedOff).toBe('wait');
    expect(toRequest(draft({ loggedOff: 'service-account' })).loggedOff).toBe('service-account');
  });
});

describe('RegisterWizard', () => {
  let mock: MockBackend;
  
  function mount(options: Parameters<typeof makeMock>[0] = {}) {
    mock = makeMock(options);
    TestBed.configureTestingModule({ providers: [...appProviders(mock), { provide: DIALOG_REF, useValue: { answer: vi.fn(), dismiss: vi.fn() } }] });
    const f = TestBed.createComponent(RegisterWizard);
    f.detectChanges();
    return { f, el: f.nativeElement as HTMLElement };
  }
  afterEach(() => {
    mock.dispose();
  });

  const input = (el: HTMLElement, sel: string) => el.querySelector(sel) as HTMLInputElement;
  type Wizard = ReturnType<typeof mount>['f'];
  const signal = new AbortController().signal;
  /** The footer's declared actions, as the frame would lay them out. */
  const actions = (f: Wizard) => f.componentInstance.dialogActions();
  /** Runs the declared action `id` the way the frame does on its click. */
  const act = (f: Wizard, id: string) => {
    const a = actions(f).find((x) => x.id === id);
    if (!a) throw new Error(`no action ${id}`);
    return a.kind === 'run' ? a.run(signal) : a.kind === 'answer' ? a.answer(signal) : undefined;
  };

  it('blocks Continue with messages until the step is valid', () => {
    const { f, el } = mount();
    expect(text(el.querySelector('header.top'))).toContain('Pitcrew registers it with your server');
    void act(f, 'continue');
    f.detectChanges();
    expect(el.querySelectorAll('.errors li')).toHaveLength(2);
    typeInto(input(el, '#reg-url'), 'https://git.example.com');
    typeInto(input(el, '#reg-token'), 'abcdefgh');
    void act(f, 'continue');
    f.detectChanges();
    expect(el.querySelector('.errors')).toBeNull();
    expect(el.querySelector('#reg-name')).not.toBeNull();
    expect(el.querySelector('li.current')?.getAttribute('aria-current')).toBe('step');
  });

  async function toReview(f: ReturnType<typeof mount>['f'], el: HTMLElement) {
    typeInto(input(el, '#reg-url'), 'https://git.example.com/');
    typeInto(input(el, '#reg-token'), 'abcdefgh');
    (input(el, '.check input') as HTMLInputElement).click();
    void act(f, 'continue');
    f.detectChanges();
    typeInto(input(el, '#reg-name'), 'New-Runner');
    typeInto(input(el, '#reg-display'), 'Shown');
    const lab = input(el, '#reg-labels');
    lab.value = 'windows';
    lab.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }));
    lab.value = 'x64,';
    lab.dispatchEvent(new KeyboardEvent('keydown', { key: ',', cancelable: true }));
    f.detectChanges();
    void act(f, 'continue');
    f.detectChanges();
    void act(f, 'continue');
    f.detectChanges();
  }

  it('edits labels with the keyboard and removes them', () => {
    const { f, el } = mount();
    typeInto(input(el, '#reg-url'), 'https://git.example.com');
    typeInto(input(el, '#reg-token'), 'abcdefgh');
    void act(f, 'continue');
    f.detectChanges();
    const lab = input(el, '#reg-labels');
    for (const v of ['a', 'b', 'a']) {
      lab.value = v;
      lab.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', cancelable: true }));
    }
    f.detectChanges();
    expect(Array.from(el.querySelectorAll('.chip')).map((c) => text(c))).toEqual(['a', 'b']);
    lab.dispatchEvent(new KeyboardEvent('keydown', { key: 'Backspace', cancelable: true }));
    f.detectChanges();
    expect(el.querySelectorAll('.chip')).toHaveLength(1);
    (el.querySelector('.chip button') as HTMLButtonElement).click();
    f.detectChanges();
    expect(el.querySelectorAll('.chip')).toHaveLength(0);
    lab.value = 'blur-added';
    lab.dispatchEvent(new Event('blur'));
    f.detectChanges();
    expect(text(el.querySelector('.chip'))).toBe('blur-added');
  });

  it('shows session warnings, a folder input, a review and registers', async () => {
    const { f, el } = mount();
    const register = vi.spyOn(mock, 'register');
    await toReview(f, el);
    // Step "Where it runs" was passed with defaults; the review shows the draft.
    const review = text(el.querySelector('.review'));
    expect(review).toContain('https://git.example.com/');
    expect(review).toContain('new-runner (“Shown” on the server)');
    expect(review).toContain('windows, x64');
    expect(review).toContain('remembered in Windows Credential Manager');
    expect(review).toContain('waits for log-on');
    const answer = act(f, 'register');
    await settle(f);
    expect(register).toHaveBeenCalledWith(expect.objectContaining({ runner: 'new-runner', serverUrl: 'https://git.example.com', displayName: 'Shown', labels: ['windows', 'x64'], rememberToken: true }));
    expect(TestBed.inject(NoticeLog).notices.at(-1)?.message).toContain('Registered new-runner');
    expect(await answer).toBe(true);
  });

  it('offers adopt folder and session choices on the third step', () => {
    const { f, el } = mount();
    typeInto(input(el, '#reg-url'), 'https://git.example.com');
    typeInto(input(el, '#reg-token'), 'abcdefgh');
    void act(f, 'continue');
    f.detectChanges();
    typeInto(input(el, '#reg-name'), 'runner-two');
    f.detectChanges();
    void act(f, 'continue');
    f.detectChanges();
    expect(el.querySelector('input[aria-label="Folder to adopt"]')).toBeNull();
    expect(el.querySelector('.callout.warning')).toBeNull();
    (el.querySelector('input[name="folder"][type="radio"]:not(:checked)') as HTMLInputElement).click();
    f.detectChanges();
    void act(f, 'continue');
    f.detectChanges();
    expect(text(el.querySelector('.errors'))).toContain('Enter the folder');
    typeInto(input(el, 'input[aria-label="Folder to adopt"]'), 'C:\\runners\\x');
    (el.querySelectorAll('input[name="off"]')[1] as HTMLInputElement).click();
    f.detectChanges();
    expect(text(el.querySelector('.callout.warning'))).toContain('Jobs that run while nobody is logged on');
    (el.querySelectorAll('input[name="session"]')[1] as HTMLInputElement).click();
    f.detectChanges();
    expect(el.querySelector('input[name="off"]')).toBeNull();
    expect(text(el.querySelector('.callout.warning'))).toContain('Every job');
    void act(f, 'continue');
    f.detectChanges();
    expect(text(el.querySelector('.review'))).toContain('Adopt C:\\runners\\x');
    expect(text(el.querySelector('.review'))).toContain('Service session');
  });

  it('goes back and jumps to earlier steps only', () => {
    const { f, el } = mount();
    typeInto(input(el, '#reg-url'), 'https://git.example.com');
    typeInto(input(el, '#reg-token'), 'abcdefgh');
    void act(f, 'continue');
    f.detectChanges();
    const steps = Array.from(el.querySelectorAll('.stepper button')) as HTMLButtonElement[];
    expect(steps.map((b) => b.disabled)).toEqual([false, true, true, true]);
    steps[0]!.click();
    f.detectChanges();
    expect(el.querySelector('#reg-url')).not.toBeNull();
    void act(f, 'continue');
    f.detectChanges();
    void act(f, 'back');
    f.detectChanges();
    expect(el.querySelector('#reg-url')).not.toBeNull();
    expect(actions(f).map((a) => a.id)).toEqual(['cancel', 'continue']);
  });

  it('refuses with the failure (the frame shows it and stays open) when registration fails', async () => {
    const { f, el } = mount();
    typeInto(input(el, '#reg-url'), 'https://git.example.com');
    typeInto(input(el, '#reg-token'), 'short');
    void act(f, 'continue');
    f.detectChanges();
    typeInto(input(el, '#reg-name'), 'ok-name');
    f.detectChanges();
    void act(f, 'continue');
    f.detectChanges();
    void act(f, 'continue');
    f.detectChanges();
    const failed = act(f, 'register');
    await expect(failed).rejects.toThrow(/registration token/);
    await settle(f);
    expect(actions(f).find((a) => a.id === 'register')?.enabled).not.toBe(false);
  });

  it('declares Cancel as the dismissal and names the Keychain on macOS', async () => {
    const { f, el } = mount({ platform: 'macos' });
    expect(text(el)).toContain('macOS Keychain');
    expect(actions(f).find((a) => a.id === 'cancel')?.kind).toBe('dismiss');
    expect(el.querySelector('footer')).toBeNull();
    await flush();
  });
});
