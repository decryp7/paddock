import { answered, DISMISSED } from '@decryp7/paddock-ui/shell';
import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  CHANNELS_REMOVE_DIALOG_ID,
  CHANNELS_SECRET_DIALOG_ID,
  CHANNEL_EVENTS,
  MAX_CHANNELS,
  MemoryChannelsBackend,
  provideShellChannels,
  type ChannelConfig,
  type ChannelEventDecl,
  type MemoryChannelsOptions,
} from '@decryp7/paddock-ui/shell/channels';
import { DialogService } from '@decryp7/paddock-ui/shell/dialogs';
import { PdChannelsPage } from './channels-page';

const WEBHOOK: ChannelConfig = {
  id: 'slack',
  name: 'Slack',
  enabled: true,
  events: ['diskLow', 'jobFailed', 'serviceDown'],
  minSeverity: 'warning',
  allowLan: false,
  transport: { type: 'webhook', url: 'https://hooks.example.com/services/abc', auth: true },
};
const NTFY: ChannelConfig = {
  id: 'phone',
  name: 'Phone',
  enabled: false,
  events: [],
  minSeverity: 'error',
  allowLan: false,
  transport: { type: 'ntfy', server: 'https://ntfy.sh', topic: 'my_topic', auth: false },
};
const EMAIL: ChannelConfig = {
  id: 'mail',
  name: 'Mail',
  enabled: true,
  events: ['diskLow'],
  minSeverity: 'info',
  allowLan: false,
  transport: { type: 'email', host: 'smtp.example.com', port: 587, security: 'startTls', username: 'me', from: 'app@example.com', to: ['me@example.com'] },
};

/** Lets the simulated engine and the stores settle (microtasks only). */
async function settle(rounds = 12): Promise<void> {
  for (let i = 0; i < rounds; i++) await Promise.resolve();
}

const DECLARED: readonly ChannelEventDecl[] = [
  { name: 'diskLow', group: 'Storage', description: 'A drive is nearly full.' },
  { name: 'jobFailed', group: 'Jobs', description: 'A background job ended with an error.' },
  { name: 'jobDone', group: 'Jobs', description: 'A background job finished.' },
];

const text = (el: Element): string => (el.textContent ?? '').replace(/\s+/g, ' ').trim();

/** The dialog service as the page uses it: records what was opened and answers like the secret dialog (which stores the secret). */
class FakeDialogs {
  readonly opened: { id: string; inputs: Record<string, unknown> }[] = [];
  answer: boolean | undefined = true;
  /** What the remove confirmation answers: true is Remove. */
  removeAnswer: boolean | undefined = true;
  constructor(private readonly memory: MemoryChannelsBackend) {}
  open(id: string, options: { inputs?: Record<string, unknown> } = {}) {
    const inputs = options.inputs ?? {};
    this.opened.push({ id, inputs });
    const closed = (async () => {
      if (id === CHANNELS_REMOVE_DIALOG_ID) return this.removeAnswer === undefined ? DISMISSED : answered(this.removeAnswer);
      if (this.answer) await this.memory.setSecret(String(inputs['channelId']), 'typed-elsewhere');
      return this.answer === undefined ? DISMISSED : answered(this.answer);
    })();
    return { closed, dismiss: () => undefined };
  }
}

/** The engine's sends wait until `all()`: a test sees "Still working". */
function held(options: MemoryChannelsOptions = {}) {
  const queue: (() => void)[] = [];
  const memory = new MemoryChannelsBackend({ schedule: (step) => queue.push(step), ...options });
  return {
    memory,
    pending: () => queue.length,
    async next(): Promise<void> {
      queue.shift()?.();
      await settle();
    },
    async all(): Promise<void> {
      while (queue.length) queue.shift()?.();
      await settle();
    },
  };
}

describe('the Notification channels page', () => {
  async function open(options: MemoryChannelsOptions = {}, events: readonly ChannelEventDecl[] = []) {
    const h = held({ channels: [WEBHOOK, NTFY], secrets: ['slack'], ...options });
    const dialogs = new FakeDialogs(h.memory);
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideShellChannels({ backend: () => h.memory }), ...(events.length ? [{ provide: CHANNEL_EVENTS, useValue: events }] : []), { provide: DialogService, useValue: dialogs }],
    });
    const fixture = TestBed.createComponent(PdChannelsPage);
    const el = fixture.nativeElement as HTMLElement;
    const flush = async (): Promise<void> => {
      await settle();
      await fixture.whenStable();
      await settle();
      await fixture.whenStable();
    };
    await flush();
    const q = (id: string): HTMLElement | null => el.querySelector(`[data-testid="${id}"]`);
    const click = async (target: string | HTMLElement | null): Promise<void> => {
      const node = typeof target === 'string' ? q(target) : target;
      // A Material toggle or checkbox is a host: the user clicks the real control inside it.
      ((node as HTMLElement).querySelector<HTMLElement>('input, button[role="switch"]') ?? (node as HTMLElement)).click();
      await flush();
    };
    const type = async (id: string, value: string): Promise<void> => {
      const input = q(id) as HTMLInputElement | HTMLTextAreaElement;
      input.value = value;
      input.dispatchEvent(new Event('input', { bubbles: true }));
      input.dispatchEvent(new Event('blur'));
      await flush();
    };
    const row = (id: string): HTMLElement => q(`channel-row-${id}`) as HTMLElement;
    const rowButton = (id: string, testid: string): HTMLElement => row(id).querySelector(`[data-testid="${testid}"]`) as HTMLElement;
    const choose = async (id: string, label: string): Promise<void> => {
      (el.querySelector(`#${id}`) as HTMLElement).click();
      await flush();
      const option = [...document.querySelectorAll<HTMLElement>('.cdk-overlay-container [role=option]')].find((o) => text(o) === label);
      option!.click();
      await flush();
    };
    return { fixture, el, q, click, type, row, rowButton, choose, flush, h, dialogs, memory: h.memory };
  }

  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = '')));

  it('shows a skeleton while it loads, then one table row per channel with its state', async () => {
    const p = await open();
    expect(p.q('channels-loading')).toBeNull();
    const rows = [...p.el.querySelectorAll('tbody tr')];
    expect(rows).toHaveLength(2);
    expect([...p.el.querySelectorAll('thead th')].map((th) => text(th))).toEqual(['Name', 'Type', 'Events', 'State', 'Actions']);
    expect(text(p.row('slack'))).toContain('Slack');
    expect(text(p.row('slack'))).toContain('Webhook');
    expect(text(p.row('slack'))).toContain('diskLow, jobFailed and 1 more');
    expect(text(p.row('slack'))).toContain('Warnings and errors');
    expect(text(p.row('slack').querySelector('pd-status-pill')!)).toBe('On');
    // The secret is stored: no warning pill; the second channel is off and needs none.
    expect(p.row('slack').querySelectorAll('pd-status-pill')).toHaveLength(1);
    expect(text(p.row('phone').querySelector('pd-status-pill')!)).toBe('Off');
    expect(text(p.row('phone'))).toContain('No events chosen');
  });

  it('paints a skeleton with a name until the engine answers', async () => {
    const h = held({ channels: [WEBHOOK] });
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), provideShellChannels({ backend: () => h.memory }), { provide: DialogService, useValue: new FakeDialogs(h.memory) }],
    });
    const fixture = TestBed.createComponent(PdChannelsPage);
    fixture.detectChanges();
    const skeleton = (fixture.nativeElement as HTMLElement).querySelector('[data-testid="channels-loading"]');
    expect(skeleton?.getAttribute('aria-label')).toContain('Reading');
    await settle();
    await fixture.whenStable();
    expect((fixture.nativeElement as HTMLElement).querySelector('[data-testid="channels-loading"]')).toBeNull();
  });

  it('warns when a channel needs a secret and none is stored', async () => {
    const p = await open({ secrets: [], channels: [WEBHOOK, EMAIL] });
    expect(text(p.row('slack').querySelectorAll('pd-status-pill')[1]!)).toBe('No token');
    expect(text(p.row('mail').querySelectorAll('pd-status-pill')[1]!)).toBe('No password');
  });

  it('has one primary button, icon buttons with names and tooltips, and a labelled table', async () => {
    const p = await open();
    expect(p.el.querySelectorAll('button[matButton="filled"], button.mat-mdc-unelevated-button')).toHaveLength(1);
    expect(text(p.q('channel-add')!)).toBe('Add channel');
    const edit = p.rowButton('slack', 'channel-edit');
    const test = p.rowButton('slack', 'channel-test');
    const remove = p.rowButton('slack', 'channel-remove');
    expect([edit, test, remove].map((b) => b.getAttribute('aria-label'))).toEqual(['Edit Slack', 'Send a test to Slack', 'Remove Slack']);
    for (const b of [edit, test, remove]) {
      expect(b.getAttribute('aria-label')).not.toBe('');
      expect(b.hasAttribute('title')).toBe(false);
      expect(b.querySelector('pd-glyph')).not.toBeNull();
    }
    expect(p.el.querySelector('section')?.getAttribute('aria-labelledby')).toBe('pd-ch-title');
    expect(p.el.querySelector('table caption')).not.toBeNull();
    expect(p.el.querySelector('pd-path-text')?.getAttribute('class')).toContain('pd-ch__muted');
  });

  it('shows the empty state with the one Add channel button', async () => {
    const p = await open({ channels: [] });
    expect(p.q('channels-empty')).not.toBeNull();
    expect(p.q('channels-table')).toBeNull();
    expect(p.q('channel-add')).toBeNull();
    expect(p.el.querySelectorAll('button[matButton="filled"], button.mat-mdc-unelevated-button')).toHaveLength(1);
    await p.click('channel-add-empty');
    expect(p.q('channel-editor')).not.toBeNull();
    expect(document.activeElement).toBe(p.q('channel-name'));
  });

  it('shows a failed load as an inline error and Retry loads the list', async () => {
    const p = await open({ getFailures: 1 });
    expect(p.q('channels-error')?.getAttribute('role')).toBe('alert');
    expect(text(p.q('channels-error')!)).toContain('could not be read');
    await p.click(p.q('channels-error')!.querySelector('.pd-retry') as HTMLElement);
    expect(p.q('channels-error')).toBeNull();
    expect(p.el.querySelectorAll('tbody tr')).toHaveLength(2);
  });

  it('adds a webhook: hints until it is complete, the secret dialog, then Save', async () => {
    const p = await open({ channels: [], secrets: [] });
    await p.click('channel-add-empty');
    expect((p.q('channel-save') as HTMLButtonElement).disabled).toBe(true);
    // No red on an untouched form; a left, empty field shows its hint.
    expect(p.q('channel-editor')!.querySelector('.pd-ch__hint--bad')).toBeNull();
    await p.type('channel-name', '');
    expect(text(p.q('channel-editor')!)).toContain('Give the channel a name.');
    expect(p.q('channel-name')!.getAttribute('aria-invalid')).toBe('true');
    await p.type('channel-name', 'My Slack');
    await p.type('channel-url', 'nope');
    expect(text(p.q('channel-editor')!)).toContain('starting with https://');
    await p.type('channel-url', 'https://hooks.example.com/x');
    expect((p.q('channel-save') as HTMLButtonElement).disabled).toBe(false);

    // The channel needs a token: no secret yet, and the dialog sets it for the id the channel will have.
    expect(p.q('channel-secret')).toBeNull();
    await p.click('channel-auth');
    expect(text(p.q('channel-secret-state')!)).toBe('No secret yet');
    expect(text(p.q('channel-secret-set')!)).toBe('Set token…');
    expect(p.q('channel-secret-clear')).toBeNull();
    await p.click('channel-secret-set');
    expect(p.dialogs.opened).toEqual([{ id: CHANNELS_SECRET_DIALOG_ID, inputs: { channelId: 'My-Slack', label: 'My Slack', kind: 'token' } }]);
    expect(text(p.q('channel-secret-state')!)).toBe('Secret stored');
    expect(text(p.q('channel-secret-set')!)).toBe('Change token…');
    // The page never sees the secret.
    expect(p.el.innerHTML).not.toContain('typed-elsewhere');

    // Renaming now does not change the id the secret was stored under.
    await p.type('channel-name', 'Renamed');
    await p.click('channel-save');
    expect(p.q('channel-editor')).toBeNull();
    expect(p.row('My-Slack')).toBeTruthy();
    expect(text(p.row('My-Slack'))).toContain('Renamed');
    expect(p.row('My-Slack').querySelectorAll('pd-status-pill')).toHaveLength(1);
    expect(p.memory.secretOf('My-Slack')).toBe('typed-elsewhere');
  });

  it('shows the engine’s refusal of a save as it is and keeps the editor open', async () => {
    const p = await open({ channels: [WEBHOOK], secrets: ['slack'], saveError: { code: 'invalid', message: 'The address must start with https://.', retryable: false } });
    await p.click(p.rowButton('slack', 'channel-edit'));
    expect(text(p.q('channel-editor')!)).toContain('Edit channel');
    await p.click('channel-save');
    const error = p.q('channel-save-error')!;
    expect(error.getAttribute('role')).toBe('alert');
    expect(text(error)).toContain('The address must start with https://.');
    expect(p.q('channel-editor')).not.toBeNull();
    await p.click('channel-cancel');
    expect(p.q('channel-editor')).toBeNull();
  });

  it('edits a channel in place, switching its type shows the fields of that type', async () => {
    const p = await open();
    await p.click(p.rowButton('phone', 'channel-edit'));
    expect(p.q('channel-topic')).not.toBeNull();
    expect((p.q('channel-name') as HTMLInputElement).value).toBe('Phone');
    expect(p.row('phone').classList).toContain('pd-ch__editing');
    await p.type('channel-topic', 'other_topic');
    await p.type('channel-events', 'diskLow, jobFailed\ndiskLow');
    await p.choose('pd-ch-severity', 'Info and above');
    await p.click('channel-enabled');
    await p.click('channel-save');
    const phone = p.memory.secretOf('phone');
    expect(phone).toBeUndefined();
    expect(text(p.row('phone'))).toContain('diskLow, jobFailed');
    expect(text(p.row('phone'))).toContain('Info and above');
    expect(text(p.row('phone').querySelector('pd-status-pill')!)).toBe('On');
    expect(p.el.querySelectorAll('tbody tr')).toHaveLength(2);

    await p.click('channel-add');
    await p.choose('pd-ch-type', 'Email');
    expect(p.q('channel-host')).not.toBeNull();
    expect(p.q('channel-auth')).toBeNull();
    await p.type('channel-name', 'Ops');
    await p.type('channel-host', 'smtp.example.com');
    await p.type('channel-from', 'app@example.com');
    await p.type('channel-to', 'a@example.com, b@example.com');
    await p.choose('pd-ch-security', 'TLS (implicit)');
    expect((p.q('channel-save') as HTMLButtonElement).disabled).toBe(false);
    // A user name makes it need a password.
    await p.type('channel-username', 'ops');
    expect(text(p.q('channel-secret-set')!)).toBe('Set password…');
    await p.click('channel-save');
    const saved = (await p.memory.get()).settings.channels.find((c) => c.id === 'Ops');
    expect(saved?.transport).toMatchObject({ type: 'email', port: 587, security: 'implicit', username: 'ops', to: ['a@example.com', 'b@example.com'] });
    expect(text(p.row('Ops').querySelectorAll('pd-status-pill')[1]!)).toBe('No password');
  });

  it('removes a secret from the editor, and a cancelled new channel does not leave one behind', async () => {
    const p = await open();
    await p.click(p.rowButton('slack', 'channel-edit'));
    expect(text(p.q('channel-secret-state')!)).toBe('Secret stored');
    await p.click('channel-secret-clear');
    expect(text(p.q('channel-secret-state')!)).toBe('No secret yet');
    expect(p.memory.secretOf('slack')).toBeUndefined();
    await p.click('channel-cancel');

    await p.click('channel-add');
    await p.type('channel-name', 'Temp');
    await p.click('channel-auth');
    await p.click('channel-secret-set');
    expect(p.memory.secretOf('Temp')).toBe('typed-elsewhere');
    await p.click('channel-cancel');
    expect(p.memory.secretOf('Temp')).toBeUndefined();

    // A dismissed dialog stores nothing.
    p.dialogs.answer = undefined;
    await p.click('channel-add');
    await p.type('channel-name', 'Other');
    await p.click('channel-auth');
    await p.click('channel-secret-set');
    expect(text(p.q('channel-secret-state')!)).toBe('No secret yet');
  });

  it('sends a test of the draft: Still working for as long as it runs, then Delivered', async () => {
    const p = await open({ channels: [], secrets: [] });
    await p.click('channel-add-empty');
    expect((p.q('channel-send-test') as HTMLButtonElement).disabled).toBe(true);
    await p.type('channel-name', 'Hook');
    await p.type('channel-url', 'https://hooks.example.com/x');
    await p.click('channel-send-test');
    const result = p.q('channel-test-result-Hook')!;
    expect(result.getAttribute('role')).toBe('status');
    expect(text(result)).toContain('Still working…');
    expect((p.q('channel-send-test') as HTMLButtonElement).disabled).toBe(true);
    // Nothing was saved by testing.
    expect((await p.memory.get()).settings.channels).toEqual([]);
    await p.h.all();
    await p.flush();
    expect(text(p.q('channel-test-result-Hook')!)).toBe('Delivered');
    expect(p.q('channel-test-result-Hook')!.className).toContain('pd-ch__test--ok');
    expect((p.q('channel-send-test') as HTMLButtonElement).disabled).toBe(false);
  });

  it('shows the engine’s advice when a test fails, also for an unsaved draft that needs a secret', async () => {
    const p = await open({ channels: [], secrets: [] });
    await p.click('channel-add-empty');
    await p.type('channel-name', 'Hook');
    await p.type('channel-url', 'https://hooks.example.com/x');
    await p.click('channel-auth');
    await p.click('channel-send-test');
    await p.h.all();
    await p.flush();
    const result = p.q('channel-test-result-Hook')!;
    expect(result.getAttribute('role')).toBe('alert');
    expect(text(result)).toContain('none is stored yet');
    expect(result.className).toContain('pd-ch__test--bad');
  });

  it('tests a saved channel from its row, and a rejected one says why next to the name', async () => {
    const p = await open({ outcome: 'rejected' });
    await p.click(p.rowButton('slack', 'channel-test'));
    expect(text(p.q('channel-test-result-slack')!)).toContain('Still working');
    expect((p.rowButton('slack', 'channel-test') as HTMLButtonElement).disabled).toBe(true);
    await p.h.all();
    await p.flush();
    expect(text(p.q('channel-test-result-slack')!)).toContain('HTTP 401');
    expect(p.q('channel-test-result-slack')!.getAttribute('role')).toBe('alert');
    expect(p.row('slack').contains(p.q('channel-test-result-slack'))).toBe(true);
  });

  it('shows the attempt and phase of a running test, then the attempts of a delivered one', async () => {
    const p = await open({ outcome: 'retry' });
    await p.click(p.rowButton('slack', 'channel-test'));
    const progress = () => text(p.q('channel-test-progress-slack')!);
    await p.h.next();
    await p.flush();
    expect(progress()).toBe('Attempt 1 of 3 · sending…');
    await p.h.next();
    await p.flush();
    expect(progress()).toBe('Attempt 1 of 3 · waiting 4 s before the next try');
    await p.h.next();
    await p.flush();
    expect(progress()).toBe('Attempt 2 of 3 · sending…');
    expect(text(p.q('channel-test-result-slack')!)).toContain('Still working');
    await p.h.all();
    await p.flush();
    expect(text(p.q('channel-test-result-slack')!)).toBe('Delivered after 2 attempts');
    expect(p.q('channel-test-progress-slack')).toBeNull();
  });

  it('keeps saying Still working, with attempt 1 sending, for a hanging test', async () => {
    const hang = await open({ outcome: 'hang' });
    await hang.click(hang.rowButton('slack', 'channel-test'));
    await hang.h.all();
    await hang.flush();
    expect(text(hang.q('channel-test-result-slack')!)).toContain('Still working');
    expect(text(hang.q('channel-test-progress-slack')!)).toBe('Attempt 1 of 3 · sending…');
    expect((hang.rowButton('slack', 'channel-test') as HTMLButtonElement).disabled).toBe(true);
  });

  it('removes a channel after the remove dialog answers Remove, and Keep or a dismissal leaves it', async () => {
    const p = await open();
    p.dialogs.removeAnswer = false;
    await p.click(p.rowButton('slack', 'channel-remove'));
    expect(p.dialogs.opened.at(-1)).toEqual({ id: CHANNELS_REMOVE_DIALOG_ID, inputs: { channelId: 'slack', name: 'Slack' } });
    expect(p.el.querySelectorAll('tbody tr')).toHaveLength(2);
    p.dialogs.removeAnswer = undefined;
    await p.click(p.rowButton('slack', 'channel-remove'));
    expect(p.el.querySelectorAll('tbody tr')).toHaveLength(2);
    // No inline confirmation any more: the question is the dialog's.
    expect(p.q('channel-remove-confirm')).toBeNull();
    p.dialogs.removeAnswer = true;
    await p.click(p.rowButton('slack', 'channel-remove'));
    expect(p.el.querySelectorAll('tbody tr')).toHaveLength(1);
    expect(p.memory.secretOf('slack')).toBeUndefined();
    // Removing the last one leads to the empty state.
    await p.click(p.rowButton('phone', 'channel-remove'));
    expect(p.q('channels-empty')).not.toBeNull();
  });

  it('closes the editor of a channel that is removed', async () => {
    const p = await open();
    await p.click(p.rowButton('slack', 'channel-edit'));
    await p.click(p.rowButton('slack', 'channel-remove'));
    expect(p.q('channel-editor')).toBeNull();
  });

  it('disables Add channel at the limit and says why', async () => {
    const many = Array.from({ length: MAX_CHANNELS }, (_, i) => ({ ...NTFY, id: `c${i}`, name: `Channel ${i}` }));
    const p = await open({ channels: many, secrets: [] });
    const add = p.q('channel-add')!;
    expect(add.getAttribute('aria-disabled')).toBe('true');
    expect(add.getAttribute('aria-describedby')).toBe('pd-ch-limit');
    expect(text(p.q('channels-limit')!)).toContain(`${MAX_CHANNELS}`);
    await p.click(add);
    expect(p.q('channel-editor')).toBeNull();
    await p.click(p.rowButton('c0', 'channel-remove'));
    expect(p.q('channel-add')!.getAttribute('aria-disabled')).toBeNull();
    expect(p.q('channels-limit')).toBeNull();
  });

  it('is keyboard operable: Enter in a field saves, Escape cancels without leaving the page’s window', async () => {
    const p = await open();
    await p.click('channel-add');
    await p.type('channel-name', 'Keys');
    await p.type('channel-url', 'https://hooks.example.com/k');
    const escape = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true });
    const outer = vi.fn();
    p.el.addEventListener('keydown', outer);
    p.q('channel-name')!.dispatchEvent(escape);
    await p.flush();
    expect(p.q('channel-editor')).toBeNull();
    // Escape in the editor stays in the editor: the surrounding window does not close.
    expect(outer).not.toHaveBeenCalled();

    await p.click('channel-add');
    await p.type('channel-name', 'Keys');
    await p.type('channel-url', 'https://hooks.example.com/k');
    p.q('channel-editor')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await p.flush();
    expect(p.q('channel-editor')).toBeNull();
    expect(p.row('Keys')).toBeTruthy();
    // Every control has a name: fields are labelled, switches say what they switch.
    await p.click('channel-add');
    for (const input of p.q('channel-editor')!.querySelectorAll('input, textarea, button')) {
      const name = input.getAttribute('aria-label') ?? text(input.closest('label, mat-slide-toggle') ?? input) ?? '';
      expect(name, input.outerHTML).not.toBe('');
    }
    expect(p.q('channel-enabled')!.querySelector('button')!.getAttribute('role')).toBe('switch');
  });

  it('keeps the page working when the editor is opened over a failed save and dismisses the old failure', async () => {
    const p = await open({ saveError: { code: 'busy', message: 'The engine is busy.', retryable: true } });
    await p.click(p.rowButton('slack', 'channel-edit'));
    await p.click('channel-save');
    expect(p.q('channel-save-error')).not.toBeNull();
    await p.click('channel-cancel');
    await p.click(p.rowButton('phone', 'channel-edit'));
    expect(p.q('channel-save-error')).toBeNull();
  });

  it('shows the home-network checkbox with plain words and helper text, and saves it with the channel', async () => {
    const p = await open();
    await p.click(p.rowButton('phone', 'channel-edit'));
    const box = p.q('channel-allow-lan')!;
    expect(text(box)).toBe('This channel is on my home network (allow a private address)');
    expect(box.querySelector('input')!.getAttribute('aria-describedby')).toBe('pd-ch-h-lan');
    expect(text(p.el.querySelector('#pd-ch-h-lan')!)).toContain('Turn this on only for a web address, ntfy server or mail server in your own home or office network');
    expect(box.querySelector('input')!.checked).toBe(false);
    await p.type('channel-server', 'http://ntfy.lan');
    await p.click('channel-allow-lan');
    await p.click('channel-save');
    const saved = (await p.memory.get()).settings.channels.find((c) => c.id === 'phone');
    expect(saved?.allowLan).toBe(true);
    expect(saved?.transport).toMatchObject({ server: 'http://ntfy.lan' });
  });

  it('shows the checkbox for webhook and email channels, and a private address is refused without it', async () => {
    const p = await open();
    await p.click(p.rowButton('slack', 'channel-edit'));
    expect(p.q('channel-allow-lan')).not.toBeNull();
    await p.type('channel-url', 'http://192.168.1.20/hook');
    await p.click('channel-save');
    expect(text(p.q('channel-save-error')!)).toContain('http://');
    await p.click('channel-allow-lan');
    await p.click('channel-save');
    expect(p.q('channel-editor')).toBeNull();
    await p.click('channel-add');
    await p.choose('pd-ch-type', 'Email');
    expect(p.q('channel-allow-lan')).not.toBeNull();
  });

  it('saves the switch on an email channel whose mail server is on the home network', async () => {
    const p = await open();
    await p.click('channel-add');
    await p.choose('pd-ch-type', 'Email');
    await p.type('channel-name', 'Home mail');
    await p.type('channel-host', 'mail.local');
    await p.type('channel-from', 'app@example.com');
    await p.type('channel-to', 'me@example.com');
    await p.click('channel-save');
    expect(text(p.q('channel-save-error')!)).toContain('home network');
    await p.click('channel-allow-lan');
    await p.click('channel-save');
    const saved = (await p.memory.get()).settings.channels.find((c) => c.id === 'Home-mail');
    expect(saved?.allowLan).toBe(true);
  });

  describe('with declared events', () => {
    it('lists them as checkboxes under group headings with their descriptions', async () => {
      const p = await open({}, DECLARED);
      await p.click(p.rowButton('slack', 'channel-edit'));
      expect(p.q('channel-events')).toBeNull();
      const groups = [...p.el.querySelectorAll('fieldset')].map((g) => text(g.querySelector('legend')!));
      expect(groups).toEqual(['Events', 'Storage', 'Jobs', 'Other events']);
      const disk = p.q('channel-event-diskLow')!;
      expect(text(disk)).toBe('diskLow');
      expect(disk.querySelector('input')!.type).toBe('checkbox');
      expect(disk.querySelector('input')!.checked).toBe(true);
      expect(p.q('channel-event-jobDone')!.querySelector('input')!.checked).toBe(false);
      expect(p.q('channel-event-jobDone')!.querySelector('input')!.getAttribute('aria-describedby')).toBe('pd-ch-ev-1-1');
      expect(text(p.el.querySelector('#pd-ch-ev-1-1')!)).toBe('A background job finished.');
    });

    it('toggling checkboxes changes the channel’s event names and saving keeps them', async () => {
      const p = await open({}, DECLARED);
      await p.click(p.rowButton('phone', 'channel-edit'));
      await p.click('channel-event-jobDone');
      await p.click('channel-event-diskLow');
      await p.click('channel-event-jobFailed');
      await p.click('channel-event-jobFailed');
      await p.click('channel-save');
      const saved = (await p.memory.get()).settings.channels.find((c) => c.id === 'phone');
      expect(saved?.events).toEqual(['jobDone', 'diskLow']);
    });

    it('keeps names the app did not declare as removable chips and never drops them', async () => {
      const odd: ChannelConfig = { ...NTFY, id: 'odd', name: 'Odd', events: ['diskLow', 'oldEvent', 'legacyEvent'] };
      const p = await open({ channels: [odd] }, DECLARED);
      await p.click(p.rowButton('odd', 'channel-edit'));
      const other = p.q('channel-events-other')!;
      expect([...other.querySelectorAll('li')].map((li) => text(li))).toEqual(['oldEvent', 'legacyEvent']);
      // Saving without touching them keeps them.
      await p.click('channel-event-jobDone');
      await p.click('channel-save');
      expect((await p.memory.get()).settings.channels[0]!.events).toEqual(['diskLow', 'oldEvent', 'legacyEvent', 'jobDone']);
      // Removing one is explicit, and the remove button has a name.
      await p.click(p.rowButton('odd', 'channel-edit'));
      expect(p.q('channel-event-remove-oldEvent')!.getAttribute('aria-label')).toBe('Remove event oldEvent');
      await p.click('channel-event-remove-oldEvent');
      // Focus moves to the chip that took its place, not to the page.
      expect(document.activeElement).toBe(p.q('channel-event-remove-legacyEvent'));
      await p.click('channel-event-remove-legacyEvent');
      expect(document.activeElement).toBe(p.q('channel-event-other-input'));
      await p.type('channel-event-other-input', 'legacyEvent');
      await p.click('channel-event-other-add');
      await p.click('channel-save');
      expect((await p.memory.get()).settings.channels[0]!.events).toEqual(['diskLow', 'jobDone', 'legacyEvent']);
    });

    it('adds nothing, and keeps Add disabled, while the field holds no names', async () => {
      const p = await open({}, DECLARED);
      await p.click(p.rowButton('phone', 'channel-edit'));
      const add = p.q('channel-event-other-add') as HTMLButtonElement;
      for (const typed of ['', '  ', ' , ;, ']) {
        await p.type('channel-event-other-input', typed);
        expect(add.disabled).toBe(true);
        // Enter takes the early return: no chip appears and the editor stays open.
        p.q('channel-event-other-input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
        await p.flush();
        expect(p.q('channel-events-other')!.querySelectorAll('li')).toHaveLength(0);
        expect(p.q('channel-editor')).not.toBeNull();
      }
      await p.type('channel-event-other-input', ' , real ');
      expect(add.disabled).toBe(false);
    });

    it('still takes a free-text name under Other events, and Enter adds it without saving the form', async () => {
      const p = await open({}, DECLARED);
      await p.click(p.rowButton('phone', 'channel-edit'));
      await p.type('channel-event-other-input', 'custom, second');
      await p.click('channel-event-other-add');
      expect([...p.q('channel-events-other')!.querySelectorAll('li')].map((li) => text(li))).toEqual(['custom', 'second']);
      expect((p.q('channel-event-other-input') as HTMLInputElement).value).toBe('');
      expect((p.q('channel-event-other-add') as HTMLButtonElement).disabled).toBe(true);
      await p.type('channel-event-other-input', 'third');
      p.q('channel-event-other-input')!.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
      await p.flush();
      expect(p.q('channel-editor')).not.toBeNull();
      expect([...p.q('channel-events-other')!.querySelectorAll('li')].map((li) => text(li))).toEqual(['custom', 'second', 'third']);
    });
  });

  it('keeps the free-text field as the whole control when the app declared no events', async () => {
    const p = await open();
    await p.click(p.rowButton('slack', 'channel-edit'));
    expect(p.q('channel-events')).not.toBeNull();
    expect(p.q('channel-events-picker')).toBeNull();
    expect((p.q('channel-events') as HTMLTextAreaElement).value).toBe('diskLow, jobFailed, serviceDown');
  });
});
