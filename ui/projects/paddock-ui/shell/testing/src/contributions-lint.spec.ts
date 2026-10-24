import { EnvironmentProviders, Injector, provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  defineCommand,
  defineContributionKind,
  defineDialog,
  defineHealthService,
  defineKeybinding,
  defineMenuItem,
  defineNotificationKind,
  definePage,
  defineSettingsPage,
  defineStatusItem,
  msg,
  provideContributionKinds,
  provideContributions,
  type ContributionBatch,
} from '@decryp7/paddock-ui/shell';
import { assertContributionsValid, contributionProblems } from './contributions-lint';

const load = () => Promise.resolve(class {});
const refresh = defineCommand({ id: 'sd.refresh', title: msg('sd.refresh', 'Refresh'), run: () => undefined });
const about = defineCommand({ id: 'sd.about', title: msg('sd.about', 'About'), run: () => undefined });

/** A batch with one valid item of every kind: the lint finds nothing in it. */
const VALID: ContributionBatch = {
  commands: [refresh, about],
  keybindings: [defineKeybinding({ command: 'sd.refresh', key: 'Mod+R' }), defineKeybinding({ command: 'sd.about', key: 'Mod+R', scope: 'sd.dialog' })],
  menus: [defineMenuItem({ menu: 'view', command: 'sd.refresh', order: 100 }), defineMenuItem({ menu: 'sd.models', command: 'sd.about', order: 100 })],
  pages: [definePage({ id: 'sd.models', title: msg('sd.page.models', 'Models'), icon: 'sd:model', order: 100, load, providers: () => Promise.resolve([]) })],
  statusItems: [defineStatusItem({ id: 'sd.serving', title: msg('sd.serving', 'Serving'), side: 'end', priority: 80, load })],
  settingsPages: [defineSettingsPage({ id: 'sd.engines', group: 'application', title: msg('sd.engines', 'Engines'), icon: 'sd:engine', order: 100, load })],
  dialogs: [
    defineDialog({ id: 'sd.create', kind: 'form', modality: 'modal', title: msg('sd.create', 'Create'), width: 560, minWidth: 400, height: 400, minHeight: 300, commands: ['apps_create', 'plugin:paddock-shell|about_*'], load }),
    defineDialog({ id: 'sd.confirm', kind: 'decision', requiredChoice: true, title: msg('sd.confirm', 'Confirm'), commands: [], load }),
  ],
  healthServices: [defineHealthService({ id: 'sd.engine', label: msg('sd.engine', 'the engine'), icon: 'sd:engine' })],
  notificationKinds: [defineNotificationKind({ id: 'sd.update', title: msg('sd.update', 'Update'), actions: ['install', 'later'] })],
};

function lint(...providers: EnvironmentProviders[]): string[] {
  TestBed.resetTestingModule();
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), ...providers] });
  return contributionProblems(TestBed.inject(Injector));
}

/** The problems of the valid batch plus `extra` (a batch of defects). */
const problems = (extra: Record<string, readonly unknown[]>): string[] => lint(provideContributions(VALID), provideContributions(extra as ContributionBatch));

describe('assertContributionsValid', () => {
  it('passes a window whose contributions are all valid', () => {
    expect(lint(provideContributions(VALID))).toEqual([]);
    expect(() => assertContributionsValid(TestBed.inject(Injector))).not.toThrow();
  });

  it('fails on a duplicate id (once per id, per kind)', () => {
    expect(problems({ commands: [refresh, refresh], pages: [VALID.pages![0]!] })).toEqual(['duplicate command sd.refresh', 'duplicate page sd.models']);
  });

  it('fails on a keybinding conflict in the same scope', () => {
    expect(problems({ keybindings: [defineKeybinding({ command: 'sd.about', key: 'Ctrl+R' })] })).toEqual([
      'keybinding conflict in scope window: Mod+R (sd.refresh) and Ctrl+R (sd.about) press Ctrl+R on Windows and Linux',
    ]);
    expect(problems({ keybindings: [defineKeybinding({ command: 'sd.about', key: 'mod+r' })] })).toEqual([
      'keybinding conflict in scope window: Mod+R (sd.refresh) and mod+r (sd.about) press Meta+R on macOS',
      'keybinding conflict in scope window: Mod+R (sd.refresh) and mod+r (sd.about) press Ctrl+R on Windows and Linux',
    ]);
  });

  it('fails on a menu item naming a missing command', () => {
    expect(problems({ menus: [defineMenuItem({ menu: 'file', command: 'sd.gone', order: 100 })] })).toEqual(['menu item file/sd.gone: names the missing command sd.gone']);
  });

  it('fails on a keybinding naming a missing command', () => {
    expect(problems({ keybindings: [defineKeybinding({ command: 'sd.gone', key: 'F9' })] })).toEqual(['keybinding window:F9:sd.gone: names the missing command sd.gone']);
  });

  it('fails on a page without an icon', () => {
    expect(problems({ pages: [definePage({ id: 'sd.bare', title: msg('sd.bare', 'Bare'), icon: '', order: 1, load })] })).toEqual(['page sd.bare: has no icon (namespace:name)']);
  });

  it('fails on a page without a title message id', () => {
    expect(problems({ pages: [definePage({ id: 'sd.plain', title: 'Plain' as never, icon: 'sd:x', order: 1, load })] })).toEqual([
      "page sd.plain: title has no message id (use msg('<ns>.<name>', 'Text'))",
    ]);
  });

  it('fails on a dialog without a kind', () => {
    expect(problems({ dialogs: [defineDialog({ id: 'sd.kindless', title: msg('sd.kindless', 'K'), commands: [], load } as never)] })).toEqual([
      'dialog sd.kindless: has no kind (decision, form, viewer, tool)',
    ]);
  });

  it('fails on a dialog with an unknown kind or a modality its kind forbids', () => {
    const base = { title: msg('sd.d', 'D'), commands: [], load };
    expect(
      problems({
        dialogs: [
          defineDialog({ ...base, id: 'sd.popup', kind: 'popup' as never }),
          defineDialog({ ...base, id: 'sd.modal-viewer', kind: 'viewer', modality: 'modal' }),
          defineDialog({ ...base, id: 'sd.form', kind: 'form' }),
          defineDialog({ ...base, id: 'sd.choice', kind: 'tool', requiredChoice: true }),
        ],
      }),
    ).toEqual([
      'dialog sd.popup: kind "popup" is not one of decision, form, viewer, tool',
      'dialog sd.modal-viewer: a viewer dialog cannot be modal',
      'dialog sd.form: a form dialog declares its modality (modal or modeless)',
      'dialog sd.choice: only a modal can be a required choice',
    ]);
  });

  it('fails on a min > pref layout', () => {
    expect(problems({ dialogs: [defineDialog({ id: 'sd.squeezed', kind: 'tool', title: msg('sd.s', 'S'), width: 400, minWidth: 600, height: 300, minHeight: 500, commands: [], load })] })).toEqual([
      'dialog sd.squeezed: layout min > pref: minWidth 600 exceeds width 400',
      'dialog sd.squeezed: layout min > pref: minHeight 500 exceeds height 300',
    ]);
  });

  it('fails on ids that are not namespaced', () => {
    expect(problems({ commands: [defineCommand({ id: 'Refresh', title: msg('sd.r', 'R'), run: () => undefined })] })).toEqual([
      'command Refresh: id "Refresh" is not a namespaced id (pw.models.refresh)',
    ]);
  });

  it('fails on a malformed key binding, scope or menu', () => {
    expect(
      problems({
        keybindings: [defineKeybinding({ command: 'sd.refresh', key: 'Hyper+R' }), defineKeybinding({ command: 'sd.refresh', key: 'F2', scope: 'Main Window' })],
        menus: [defineMenuItem({ menu: 'Tools', command: 'sd.refresh', group: 'A B', order: -1 })],
      }),
    ).toEqual([
      'keybinding window:Hyper+R:sd.refresh: key "Hyper+R" is not a key binding (Mod+Shift+P)',
      'keybinding Main Window:F2:sd.refresh: scope "Main Window" is malformed',
      'menu item Tools/sd.refresh: menu "Tools" is neither a shell menu (app, file, edit, view, window, help) nor a namespaced submenu',
      'menu item Tools/sd.refresh: group "A B" is malformed',
      'menu item Tools/sd.refresh: order must be a number from 0 to 100000',
    ]);
  });

  it('fails on status items, settings pages, health services and notification kinds missing their parts', () => {
    expect(
      problems({
        statusItems: [defineStatusItem({ id: 'sd.bar', title: msg('sd.bar', 'Bar'), side: 'middle' as never, priority: 2000, load: undefined as never, providers: 'x' as never })],
        settingsPages: [defineSettingsPage({ id: 'sd.prefs', group: 'misc' as never, title: msg('sd.prefs', 'Prefs'), icon: 'bad icon', order: 1, load })],
        healthServices: [defineHealthService({ id: 'sd.db', label: msg('sd.db', 'the db'), icon: '' })],
        notificationKinds: [defineNotificationKind({ id: 'sd.n', title: msg('sd.n', 'N'), actions: ['Install', 'later', 'later'] })],
        dialogs: [defineDialog({ id: 'sd.cmds', kind: 'viewer', title: msg('sd.c', 'C'), width: 0, commands: ['Apps Create'], load })],
        commands: [defineCommand({ id: 'sd.cat', title: msg('sd.cat', 'Cat'), category: msg('Cat', 'Cat'), run: undefined as never })],
      }),
    ).toEqual([
      "command sd.cat: category has no message id (use msg('<ns>.<name>', 'Text'))",
      'command sd.cat: has no run function',
      'status item sd.bar: side must be start or end',
      'status item sd.bar: priority must be a number from 0 to 1000',
      'status item sd.bar: has no load function',
      'status item sd.bar: providers is not a loader function',
      'settings page sd.prefs: group must be application, features or advanced',
      'settings page sd.prefs: has no icon (namespace:name)',
      'dialog sd.cmds: width must be a number from 1 to 10000',
      'dialog sd.cmds: command "Apps Create" is not an IPC command name',
      'health service sd.db: has no icon (namespace:name)',
      'notification kind sd.n: action "Install" is malformed',
      'notification kind sd.n: lists an action twice',
    ]);
    expect(problems({ dialogs: [{ ...VALID.dialogs![1]!, id: 'sd.nolist', commands: undefined as never }], notificationKinds: [{ id: 'sd.na', title: msg('sd.na', 'N'), actions: undefined as never }] })).toEqual([
      'dialog sd.nolist: has no commands list (an empty list is fine)',
      'notification kind sd.na: has no actions list',
    ]);
  });

  it('fails on an unknown contribution kind and on an item that is not a contribution', () => {
    expect(problems({ comands: [refresh], commands: [42] })).toEqual(['provideContributions: unknown contribution kind "comands"', 'command #3 is not a command (define it with its define* function)']);
  });

  it("runs an added kind's checks too, and reports a kind registered twice", () => {
    interface View {
      id: string;
      min: number;
      pref: number;
    }
    const views = defineContributionKind<View>({
      name: 'views',
      label: 'view',
      identity: (v) => v.id,
      check: (v) => (v.min > v.pref ? ['layout min > pref'] : []),
      checkAll: () => {
        throw new Error('broken check');
      },
    });
    const again = defineContributionKind<View>({ name: 'views', label: 'view', identity: (v) => v.id });
    expect(lint(provideContributionKinds(views, again), provideContributions({ views: [{ id: 'wb.tree', min: 300, pref: 200 }] } as ContributionBatch))).toEqual([
      'provideContributionKinds: a kind named "views" is already registered',
      'view wb.tree: layout min > pref',
      'a lint check threw: broken check',
    ]);
  });

  it('throws one error that lists every defect', () => {
    problems({ commands: [refresh], menus: [defineMenuItem({ menu: 'file', command: 'sd.gone', order: 1 })] });
    expect(() => assertContributionsValid(TestBed.inject(Injector))).toThrow(
      'Invalid contributions (2):\n- duplicate command sd.refresh\n- menu item file/sd.gone: names the missing command sd.gone',
    );
  });
});
