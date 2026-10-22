import { Dialog } from '@angular/cdk/dialog';
import { OverlayContainer } from '@angular/cdk/overlay';
import { TestBed } from '@angular/core/testing';
import { Router } from '@angular/router';
import type { RunnerSnapshot } from '../core/api.types';
import { MockBackend } from '../core/mock-backend';
import { StatusStore } from '../state/status.store';
import { appProviders, flush, makeMock, settle, text } from '../../testing/test-helpers';
import { RunnerActions } from './runner-actions';

describe('RunnerActions', () => {
  let mock: MockBackend;
  let runners: RunnerSnapshot[];

  beforeEach(async () => {
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock, [{ path: '**', children: [] }]) });
    runners = (await mock.status()).runners;
  });
  afterEach(() => {
    TestBed.inject(Dialog).closeAll();
    mock.dispose();
  });

  async function open(name: string, inputs: Record<string, unknown> = {}) {
    const f = TestBed.createComponent(RunnerActions);
    f.componentRef.setInput('runner', runners.find((r) => r.name === name));
    for (const [k, v] of Object.entries(inputs)) f.componentRef.setInput(k, v);
    await settle(f, 2);
    const trigger = f.nativeElement.querySelector('button') as HTMLButtonElement;
    expect(trigger.getAttribute('aria-label')).toBe(`Actions for ${name}`);
    trigger.click();
    await settle(f, 2);
    const overlay = TestBed.inject(OverlayContainer).getContainerElement();
    const items = () => Array.from(overlay.querySelectorAll<HTMLButtonElement>('[cdkMenuItem], .pc-menu-item'));
    const item = (label: string) => items().find((b) => text(b) === label);
    return { f, overlay, items, item };
  }

  it('greys out what does not apply to a running runner', async () => {
    const m = await open('ui-tests');
    expect(m.items().map((b) => text(b))).toEqual(['Start', 'Stop', 'Restart', 'Drain and pause', 'Processes…', 'Configuration…', 'Open logs']);
    expect(m.item('Start')!.getAttribute('aria-disabled')).toBe('true');
    expect(m.item('Stop')!.getAttribute('aria-disabled')).not.toBe('true');
  });

  it('offers Start for a crash-looping runner and Resume for a paused one', async () => {
    const a = await open('docker-lab');
    expect(a.item('Stop')!.getAttribute('aria-disabled')).toBe('true');
    expect(a.item('Start')!.getAttribute('aria-disabled')).not.toBe('true');
    TestBed.inject(OverlayContainer).ngOnDestroy();
    TestBed.resetTestingModule();
    mock = makeMock();
    TestBed.configureTestingModule({ providers: appProviders(mock, [{ path: '**', children: [] }]) });
    runners = (await mock.status()).runners;
    const b = await open('legacy-ci');
    expect(b.item('Resume')).toBeDefined();
    expect(b.item('Drain and pause')).toBeUndefined();
  });

  it('runs an action through the store', async () => {
    const m = await open('ui-tests');
    const act = vi.spyOn(TestBed.inject(StatusStore), 'act');
    m.item('Restart')!.click();
    await flush();
    expect(act).toHaveBeenCalledWith('ui-tests', 'restart');
  });

  it('opens the logs for the runner', async () => {
    const m = await open('ui-tests');
    const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
    m.item('Open logs')!.click();
    await flush();
    expect(nav).toHaveBeenCalledWith(['/logs'], { queryParams: { runner: 'ui-tests' } });
  });

  it('opens the processes and configuration of the runner', async () => {
    for (const [label, tab] of [
      ['Processes…', 'processes'],
      ['Configuration…', 'config'],
    ] as const) {
      const m = await open('ui-tests');
      const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
      m.item(label)!.click();
      await flush();
      expect(nav).toHaveBeenCalledWith(['/runners', 'ui-tests'], { queryParams: { tab } });
      nav.mockRestore();
      TestBed.inject(OverlayContainer).ngOnDestroy();
    }
  });

  it('removes a runner after confirmation and goes back to the list', async () => {
    const m = await open('legacy-ci', { allowRemove: true, small: true });
    const nav = vi.spyOn(TestBed.inject(Router), 'navigate').mockResolvedValue(true);
    m.item('Remove…')!.click();
    await flush(20);
    expect(text(document.querySelector('#confirm-title'))).toBe('Remove legacy-ci?');
    (Array.from(document.querySelectorAll('.btn')).find((b) => text(b) === 'Remove runner') as HTMLButtonElement).click();
    await flush(20);
    expect(TestBed.inject(StatusStore).runners().map((r) => r.name)).not.toContain('legacy-ci');
    expect(nav).toHaveBeenCalledWith(['/runners']);
  });

  it('does nothing when the confirmation is cancelled', async () => {
    const m = await open('legacy-ci', { allowRemove: true });
    const remove = vi.spyOn(mock, 'removeRunner');
    m.item('Remove…')!.click();
    await flush(20);
    (Array.from(document.querySelectorAll('.btn')).find((b) => text(b) === 'Cancel') as HTMLButtonElement).click();
    await flush(20);
    expect(remove).not.toHaveBeenCalled();
  });

  it('hides Remove unless allowed', async () => {
    const m = await open('ui-tests');
    expect(m.item('Remove…')).toBeUndefined();
  });

  it('disables the trigger while an action runs', async () => {
    const f = TestBed.createComponent(RunnerActions);
    f.componentRef.setInput('runner', runners[1]);
    f.detectChanges();
    const store = TestBed.inject(StatusStore);
    const p = store.act('ui-tests', 'restart');
    f.detectChanges();
    expect((f.nativeElement.querySelector('button') as HTMLButtonElement).disabled).toBe(true);
    await p;
    f.detectChanges();
    expect((f.nativeElement.querySelector('button') as HTMLButtonElement).disabled).toBe(false);
  });
});
