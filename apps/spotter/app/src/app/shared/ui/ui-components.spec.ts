import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideShellTesting, shellPorts } from '@decryp7/paddock-ui/shell/testing';
import { provideIcons } from '../icons';
import type { CommandError, WatchSummary } from '../../core/api.types';
import { idle, type Loadable } from '../../state/loadable';
import { byText, settle, text } from '../../../testing/test-helpers';
import { Confirm, deleteWatchPrompt } from './confirm';
import { KindIcon } from './kind-icon';
import { Panel } from './panel';
import { WatchStatus } from './watch-status';

@Component({
  imports: [Panel],
  template: `<sp-panel [state]="state()" label="the thing" (retry)="retries = retries + 1"><p class="content">Content</p></sp-panel>`,
})
class PanelHost {
  readonly state = signal<Loadable<string>>(idle());
  retries = 0;
}

const err: CommandError = { code: 'unavailable', message: 'Engine is down.', problems: [] };

describe('Panel', () => {
  function make() {
    const fixture = TestBed.createComponent(PanelHost);
    fixture.detectChanges();
    return fixture;
  }

  it('shows a loading placeholder until something loaded', () => {
    const f = make();
    expect(f.nativeElement.querySelector('[role=status]')).not.toBeNull();
    expect(text(f.nativeElement)).toContain('Loading the thing');
    expect(f.nativeElement.querySelector('.content')).toBeNull();
  });

  it('shows the cause with Retry when the first load failed', () => {
    const f = make();
    f.componentInstance.state.set({ value: null, loading: false, error: err });
    f.detectChanges();
    expect(text(f.nativeElement)).toContain('Could not load the thing');
    expect(text(f.nativeElement)).toContain('Engine is down.');
    expect(f.nativeElement.querySelector('.content')).toBeNull();
    (byText(f.nativeElement, 'button', /retry/i) as HTMLButtonElement).click();
    expect(f.componentInstance.retries).toBe(1);
  });

  it('shows the content, and keeps it with an inline notice when a refresh fails', () => {
    const f = make();
    f.componentInstance.state.set({ value: 'ok', loading: false, error: null });
    f.detectChanges();
    expect(f.nativeElement.querySelector('.content')).not.toBeNull();
    expect(f.nativeElement.querySelector('.stale')).toBeNull();
    f.componentInstance.state.set({ value: 'ok', loading: false, error: err });
    f.detectChanges();
    expect(f.nativeElement.querySelector('.content')).not.toBeNull();
    expect(text(f.nativeElement.querySelector('.stale'))).toContain('Could not refresh the thing: Engine is down.');
    (f.nativeElement.querySelector('.stale button') as HTMLButtonElement).click();
    expect(f.componentInstance.retries).toBe(1);
  });
});

describe('KindIcon and WatchStatus', () => {
  it('draws the icon of a kind', async () => {
    TestBed.configureTestingModule({ providers: [provideIcons()] });
    const f = TestBed.createComponent(KindIcon);
    f.componentRef.setInput('kind', 'github');
    await settle(f);
    expect(f.nativeElement.querySelector('mat-icon')?.getAttribute('data-mat-icon-name')).toMatch(/^[a-z-]+$/);
    expect(f.nativeElement.querySelector('[aria-hidden=true]')).not.toBeNull();
  });

  it.each([
    [{ status: { kind: 'ok', message: null }, paused: false, checking: false }, 'Working'],
    [{ status: { kind: 'ok', message: null }, paused: true, checking: false }, 'Paused'],
    [{ status: { kind: 'ok', message: null }, paused: true, checking: true }, 'Checking now'],
  ] as const)('shows the status word %#', (watch, word) => {
    const f = TestBed.createComponent(WatchStatus);
    f.componentRef.setInput('watch', watch as Pick<WatchSummary, 'status' | 'paused' | 'checking'>);
    f.detectChanges();
    expect(text(f.nativeElement)).toContain(word);
  });
});

describe('Confirm', () => {
  const input = deleteWatchPrompt('Acme');
  beforeEach(() => TestBed.configureTestingModule({ providers: [...provideShellTesting()] }));

  it('asks the shell for the shared pd.confirm decision and resolves true when it is confirmed', async () => {
    shellPorts().dialogAnswers.set('pd.confirm', true);
    expect(await TestBed.inject(Confirm).ask(input)).toBe(true);
    expect(shellPorts().dialogs).toEqual([
      {
        id: 'pd.confirm',
        inputs: { heading: 'Delete "Acme"?', message: 'Spotter stops watching it and forgets its history. Downloaded files stay where they are.', confirm: { label: 'Delete watch', destructive: true } },
      },
    ]);
  });

  it('resolves false when dismissed', async () => {
    expect(await TestBed.inject(Confirm).ask(input)).toBe(false);
  });
});
