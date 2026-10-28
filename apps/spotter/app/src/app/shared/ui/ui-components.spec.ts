import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { provideShellTesting, shellPorts } from '@decryp7/paddock-ui/shell/testing';
import { provideIcons } from '../icons';
import type { CommandError, WatchSummary } from '../../core/api.types';
import { idle, type Loadable } from '../../state/loadable';
import { byText, settle, text } from '../../../testing/test-helpers';
import { Confirm } from './confirm-dialog';
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
  const data = { title: 'Delete it?', message: 'This cannot be undone.', confirmLabel: 'Delete', danger: true };
  beforeEach(() => TestBed.configureTestingModule({ providers: [...provideShellTesting()] }));

  it('asks the shell for the sp.confirm decision dialog and resolves true when it is confirmed', async () => {
    shellPorts().dialogAnswers.set('sp.confirm', true);
    expect(await TestBed.inject(Confirm).ask(data)).toBe(true);
    expect(shellPorts().dialogs).toEqual([{ id: 'sp.confirm', inputs: { title: 'Delete it?', message: 'This cannot be undone.', confirmLabel: 'Delete', danger: true } }]);
  });

  it('resolves false when dismissed or declined, and defaults danger to false', async () => {
    expect(await TestBed.inject(Confirm).ask({ ...data, danger: undefined })).toBe(false);
    expect(shellPorts().dialogs[0]?.inputs).toMatchObject({ danger: false });
    shellPorts().dialogAnswers.set('sp.confirm', false);
    expect(await TestBed.inject(Confirm).ask(data)).toBe(false);
  });
});
