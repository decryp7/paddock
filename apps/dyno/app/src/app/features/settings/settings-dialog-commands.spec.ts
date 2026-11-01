import { TestBed } from '@angular/core/testing';
import { BACKEND } from '../../core/backend';
import { MockBackend } from '../../core/mock-backend';
import { TauriBackend } from '../../core/tauri-backend';
import { DIALOG_CONTRACTS } from '../../generated/dialog-contracts';
import { provideIcons } from '../../shared/icons';
import { MenuBarItems } from './menu-bar-items';
import { PopupWidgets } from './popup-widgets';

/**
 * The Settings window is a dialog window: it may call only the commands its contract declares (`DIALOG_CONTRACTS['pd.settings']`,
 * generated from Rust). The browser build's mock backend has no command names, so a component that needed a command the contract
 * leaves out passed every browser test and showed "Reading the popup layout…" for ever in the real app (the 1.0 VM test, round 2).
 * This runs the widget editors of Dyno's Settings page over the real Tauri backend with a recording `invoke`, and checks every
 * command they called against the contract.
 */
function declared(command: string): boolean {
  return DIALOG_CONTRACTS['pd.settings'].commands.some((c) => c === command || (c.endsWith('*') && command.startsWith(c.slice(0, -1))));
}

/** A Tauri backend whose commands are answered by the mock (the data shapes are the engine's), recording the names called. */
function recording(): { backend: TauriBackend; called: Set<string> } {
  const mock = new MockBackend({ latencyMs: 0, now: () => 1_700_000_000_000 });
  const called = new Set<string>();
  const call = async (cmd: string, args?: Record<string, unknown>): Promise<unknown> => {
    called.add(cmd);
    switch (cmd) {
      case 'widget_layout':
        return mock.widgetLayout(args!['surface'] as never);
      case 'edit_widget_layout':
        return mock.editWidgetLayout(args!['edit'] as never);
      case 'widget_choices':
        return mock.widgetChoices();
      case 'popover_preview':
        return mock.popoverPreview();
      case 'popover_snapshot':
        return mock.popoverSnapshot();
      case 'popover_values':
        return mock.popoverValues();
      case 'metric_readings':
        return mock.metricReadings(args!['queries'] as never);
      default:
        return null;
    }
  };
  return { backend: new TauriBackend(call as never, (() => Promise.resolve(() => undefined)) as never), called };
}

async function commandsCalledBy<T>(component: new () => T): Promise<string[]> {
  const { backend, called } = recording();
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [provideIcons(), { provide: BACKEND, useValue: backend }] });
  const f = TestBed.createComponent(component);
  f.detectChanges();
  await vi.waitFor(() => {
    f.detectChanges();
    expect((f.nativeElement as HTMLElement).querySelector('pd-widget-chooser')).not.toBeNull();
  });
  return [...called];
}

describe('the Settings window only calls the commands its dialog contract declares', () => {
  it('the popup widgets editor', async () => {
    const called = await commandsCalledBy(PopupWidgets);
    expect(called.length).toBeGreaterThan(0);
    const refused = called.filter((c) => !declared(c));
    expect(refused, `the pd.settings contract (apps/dyno/app/src-tauri/src/dialogs.rs) does not declare: ${refused.join(', ')}`).toEqual([]);
  });

  it('the menu bar items editor', async () => {
    const called = await commandsCalledBy(MenuBarItems);
    expect(called.length).toBeGreaterThan(0);
    const refused = called.filter((c) => !declared(c));
    expect(refused, `the pd.settings contract (apps/dyno/app/src-tauri/src/dialogs.rs) does not declare: ${refused.join(', ')}`).toEqual([]);
  });
});
