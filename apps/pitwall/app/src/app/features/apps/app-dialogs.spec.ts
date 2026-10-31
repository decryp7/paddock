import { TestBed } from '@angular/core/testing';
import { MatIconRegistry } from '@angular/material/icon';
import { DomSanitizer } from '@angular/platform-browser';
import { renderFramed } from '@decryp7/paddock-ui/shell/testing';
import { registerIcons } from '../../shared/icons';
import { CREATE_APP_DIALOG_ID, CreateAppDialog, MAX_APP_NAME } from './create-app-dialog';
import { KEY_SECRET_DIALOG_ID, KeySecretDialog } from './key-secret-dialog';

describe('CreateAppDialog (pw.create-app)', () => {
  const open = () => renderFramed(CreateAppDialog, { id: CREATE_APP_DIALOG_ID, kind: 'form', modality: 'modal' });
  const type = async (d: Awaited<ReturnType<typeof open>>, value: string) => {
    const input = d.el.querySelector<HTMLInputElement>('#pw-new-app-name')!;
    input.value = value;
    input.dispatchEvent(new Event('input'));
    await d.stable();
  };

  it('starts in the name field with Cancel and a disabled "Create app"', async () => {
    const d = await open();
    expect(d.actions().sort()).toEqual(['cancel', 'create']);
    expect(d.action('create')!.disabled).toBe(true);
    expect(document.activeElement).toBe(d.el.querySelector('#pw-new-app-name'));
  });

  it('a blank name keeps it disabled; a name enables it and Enter answers it trimmed', async () => {
    const d = await open();
    await type(d, '   ');
    expect(d.action('create')!.disabled).toBe(true);
    await type(d, '  Raycast ');
    expect(d.action('create')!.disabled).toBe(false);
    await d.key('Enter');
    expect(d.outcome()).toEqual({ kind: 'answered', value: 'Raycast' });
  });

  it('caps the name at the server’s limit, and Cancel answers nothing', async () => {
    const d = await open();
    expect(d.el.querySelector('#pw-new-app-name')!.getAttribute('maxlength')).toBe(String(MAX_APP_NAME));
    await d.press('cancel');
    expect(d.outcome()).toEqual({ kind: 'dismissed' });
  });
});

describe('KeySecretDialog (pw.key-secret)', () => {
  /** The shape of a real key: `pw_` and 43 characters (`pitwall-core` keys.rs). */
  const SECRET = 'pw_Xk7fQ2mR9vLc4Tn8bJ1wHs6pYd3Ge0Ua5Zi2Ko9Nq4Fr';
  const open = async () => {
    const d = await renderFramed(KeySecretDialog, { id: KEY_SECRET_DIALOG_ID, kind: 'decision', modality: 'modal', requiredChoice: true, inputs: { appName: 'Raycast', secret: SECRET, prefix: 'pw_live_8f3a' } });
    registerIcons(TestBed.inject(MatIconRegistry), TestBed.inject(DomSanitizer));
    return d;
  };
  afterEach(() => vi.restoreAllMocks());

  it('shows the key once with the warning, Copy and "I\'ve saved it", and no Cancel', async () => {
    const d = await open();
    expect(d.heading()).toBe('New key for Raycast');
    expect(d.q('key-warning')!.textContent).toContain("you won't see it again");
    const key = d.el.querySelector<HTMLElement>('#pw-key-secret')!;
    expect(key.textContent).toBe(SECRET);
    expect(key.getAttribute('role')).toBe('textbox');
    expect(key.getAttribute('aria-readonly')).toBe('true');
    // One icon: the decision's, beside the heading; the warning is a callout without an icon of its own.
    expect(d.el.querySelectorAll('mat-icon, [data-testid="dialog-icon"] svg').length).toBe(1);
    expect(d.actions().sort()).toEqual(['copy', 'saved']);
    expect(d.action('dismiss')).toBeNull();
    expect(document.activeElement).toBe(d.action('copy'));
  });

  it('Escape does nothing: only "I\'ve saved it" closes it', async () => {
    const d = await open();
    await d.key('Escape');
    expect(d.outcome()).toBeNull();
    await d.press('saved');
    expect(d.outcome()).toEqual({ kind: 'answered', value: true });
  });

  it('copies the secret and says so, without closing', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const d = await open();
    await d.press('copy');
    expect(writeText).toHaveBeenCalledWith(SECRET);
    expect(d.el.textContent).toContain('Copied to the clipboard.');
    expect(d.action('copy')!.textContent).toContain('Copy again');
    expect(d.outcome()).toBeNull();
  });

  it('says to copy by hand when the clipboard is not allowed', async () => {
    Object.defineProperty(navigator, 'clipboard', { value: { writeText: () => Promise.reject(new Error('denied')) }, configurable: true });
    const d = await open();
    await d.press('copy');
    expect(d.el.textContent).toContain('Ctrl+C');
    // The key is selected for a copy by hand.
    expect(document.getSelection()!.toString()).toBe(SECRET);
    expect(d.outcome()).toBeNull();
  });
});
