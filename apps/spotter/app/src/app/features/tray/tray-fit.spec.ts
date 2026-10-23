import { TestBed } from '@angular/core/testing';
import { TauriBackend, type CommandCall, type EventListen } from '../../core/tauri-backend';
import { TrayPage } from './tray-page';
import { appProviders, flush, makeMock, settle } from '../../../testing/test-helpers';

describe('the tray popup fits its content', () => {
  const original = globalThis.ResizeObserver;
  afterEach(() => {
    globalThis.ResizeObserver = original;
    vi.restoreAllMocks();
  });

  it('asks the shell for the popup height the page measured', async () => {
    let notify = () => undefined as void;
    globalThis.ResizeObserver = class {
      constructor(cb: () => void) {
        notify = cb;
      }
      observe = vi.fn();
      disconnect = vi.fn();
    } as unknown as typeof ResizeObserver;
    const backend = makeMock();
    const fit = vi.spyOn(backend, 'fitTray');
    TestBed.configureTestingModule({ providers: appProviders(backend) });
    const f = TestBed.createComponent(TrayPage);
    await settle(f);
    const popup = (f.nativeElement as HTMLElement).querySelector('pd-tray-popover')!;
    vi.spyOn(popup, 'getBoundingClientRect').mockReturnValue({ height: 311.4 } as DOMRect);
    notify();
    await flush(5);
    expect(fit).toHaveBeenCalledWith(312);
  });

  it('survives a shell that refuses the height', async () => {
    let notify = () => undefined as void;
    globalThis.ResizeObserver = class {
      constructor(cb: () => void) {
        notify = cb;
      }
      observe = vi.fn();
      disconnect = vi.fn();
    } as unknown as typeof ResizeObserver;
    const backend = makeMock();
    vi.spyOn(backend, 'fitTray').mockRejectedValue(new Error('no'));
    TestBed.configureTestingModule({ providers: appProviders(backend) });
    const f = TestBed.createComponent(TrayPage);
    await settle(f);
    vi.spyOn((f.nativeElement as HTMLElement).querySelector('pd-tray-popover')!, 'getBoundingClientRect').mockReturnValue({ height: 200 } as DOMRect);
    expect(() => notify()).not.toThrow();
    await flush(5);
  });

  it('maps fitTray to the fit_tray command with the height', async () => {
    const calls: [string, Record<string, unknown> | undefined][] = [];
    const call: CommandCall = <T>(cmd: string, args?: Record<string, unknown>) => {
      calls.push([cmd, args]);
      return Promise.resolve(undefined as T);
    };
    const listen: EventListen = () => Promise.resolve(() => undefined);
    await new TauriBackend(call, listen).fitTray(240);
    expect(calls).toEqual([['fit_tray', { height: 240 }]]);
  });

  it('records the call in the mock backend', async () => {
    const backend = makeMock();
    await backend.fitTray(100);
    expect(backend.calls).toContain('fitTray');
  });
});
