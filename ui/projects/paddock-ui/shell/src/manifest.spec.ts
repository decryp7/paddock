import { provideZonelessChangeDetection } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import {
  buildManifest,
  ContributionRegistry,
  defineCommand,
  defineDialog,
  defineHealthService,
  defineKeybinding,
  defineMenuItem,
  defineNotificationKind,
  definePage,
  defineSettingsPage,
  defineStatusItem,
  manifestDrift,
  msg,
  parseManifest,
  provideContributions,
  serializeManifest,
  type PaddockManifest,
} from '../public-api';

const load = () => Promise.resolve(class {});

function manifest(): PaddockManifest {
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      provideContributions({
        commands: [
          defineCommand({ id: 'sd.refresh', title: msg('sd.refresh', 'Refresh'), category: msg('sd.models', 'Models'), run: () => undefined }),
          defineCommand({ id: 'sd.about', title: msg('sd.about', 'About'), run: () => undefined }),
          defineCommand({ id: 'sd.untitled', title: 'no id' as never, run: () => undefined }),
        ],
        keybindings: [
          defineKeybinding({ command: 'sd.refresh', key: 'shift+mod+r' }),
          defineKeybinding({ command: 'sd.refresh', key: 'F5', scope: 'sd.models' }),
          defineKeybinding({ command: 'sd.about', key: 'Mod+' }),
        ],
        menus: [
          defineMenuItem({ menu: 'view', command: 'sd.refresh', group: 'models', order: 200 }),
          defineMenuItem({ menu: 'help', command: 'sd.about', order: 900 }),
          defineMenuItem({ menu: 'view', command: 'sd.about', order: 100 }),
        ],
        pages: [definePage({ id: 'sd.models', title: msg('sd.page.models', 'Models'), icon: 'sd:model', order: 200, load })],
        statusItems: [defineStatusItem({ id: 'sd.serving', title: msg('sd.serving', 'Serving'), side: 'end', priority: 80, load })],
        settingsPages: [defineSettingsPage({ id: 'sd.engines', group: 'application', title: msg('sd.engines', 'Engines'), order: 100, load })],
        dialogs: [
          defineDialog({ id: 'sd.create', kind: 'form', modality: 'modal', title: msg('sd.create', 'Create'), width: 560, minWidth: 400, commands: ['apps_list', 'apps_create', 'apps_create'], load }),
          defineDialog({ id: 'sd.about', kind: 'viewer', title: msg('sd.about.title', 'About'), instanced: true, commands: ['plugin:paddock-shell|about_*'], load }),
          defineDialog({ id: 'sd.broken', kind: 'form', title: msg('sd.broken', 'Broken'), commands: [], load }),
        ],
        healthServices: [defineHealthService({ id: 'sd.engine', label: msg('sd.engine', 'the engine'), icon: 'sd:engine' })],
        notificationKinds: [defineNotificationKind({ id: 'sd.update', title: msg('sd.update', 'Update available'), actions: ['install', 'later'] })],
      }),
    ],
  });
  return buildManifest(TestBed.inject(ContributionRegistry));
}

describe('paddock-manifest.json', () => {
  it('holds the static parts Rust checks, sorted', () => {
    const m = manifest();
    expect(m.version).toBe(1);
    expect(m.commands).toEqual([
      { id: 'sd.about', title: 'sd.about' },
      { id: 'sd.refresh', title: 'sd.refresh' },
      { id: 'sd.untitled', title: '' },
    ]);
    expect(m.keybindings).toEqual([
      { command: 'sd.refresh', key: 'F5', accelerator: 'F5', scope: 'sd.models' },
      { command: 'sd.refresh', key: 'Mod+Shift+R', accelerator: 'CmdOrCtrl+Shift+R', scope: 'window' },
    ]);
    expect(m.menus).toEqual([
      { menu: 'help', command: 'sd.about', group: 'main', order: 900, accelerator: null },
      { menu: 'view', command: 'sd.about', group: 'main', order: 100, accelerator: null },
      { menu: 'view', command: 'sd.refresh', group: 'models', order: 200, accelerator: 'CmdOrCtrl+Shift+R' },
    ]);
    expect([m.pages, m.statusItems, m.settingsPages, m.healthServices]).toEqual([['sd.models'], ['sd.serving'], ['sd.engines'], ['sd.engine']]);
    expect(m.dialogs).toEqual({
      'sd.about': { kind: 'viewer', modality: 'modeless', instanced: true, requiredChoice: false, minSize: null, commands: ['plugin:paddock-shell|about_*'] },
      'sd.create': { kind: 'form', modality: 'modal', instanced: false, requiredChoice: false, minSize: { width: 400, height: 0 }, commands: ['apps_create', 'apps_list'] },
    });
    expect(Object.keys(m.dialogs)).toEqual(['sd.about', 'sd.create']);
    expect(m.notificationKinds).toEqual({ 'sd.update': { actions: ['install', 'later'] } });
    expect(m.messages['sd.refresh']).toBe('Refresh');
    expect(m.messages['sd.models']).toBe('Models');
    expect(m.messages['sd.engine']).toBe('the engine');
    expect(Object.keys(m.messages)).toEqual([...Object.keys(m.messages)].sort());
  });

  it('round-trips: serialize, parse, serialize gives the same text and value', () => {
    const m = manifest();
    const text = serializeManifest(m);
    const parsed = parseManifest(text);
    expect(parsed.ok).toBe(true);
    if (!parsed.ok) return;
    expect(parsed.manifest).toEqual(m);
    expect(serializeManifest(parsed.manifest)).toBe(text);
    expect(text.endsWith('}\n')).toBe(true);
    expect(manifestDrift(m, text)).toEqual([]);
  });

  it('reports every reason a file is not a manifest', () => {
    expect(parseManifest('{')).toMatchObject({ ok: false });
    expect(parseManifest('[]')).toEqual({ ok: false, errors: ['the manifest is not an object'] });
    const bad = parseManifest(
      JSON.stringify({
        version: 2,
        extra: 1,
        commands: [{ id: 1, title: 'x', more: true }],
        keybindings: {},
        menus: [{ menu: 'view', command: 'x', group: 'g', order: 'first', accelerator: 3 }],
        pages: [1],
        statusItems: [],
        settingsPages: [],
        healthServices: [],
        dialogs: { a: { kind: 'popup', modality: 'modal', instanced: false, requiredChoice: false, minSize: { width: 1 }, commands: [1] }, b: 3 },
        notificationKinds: [],
        messages: { a: 1 },
      }),
    );
    expect(bad.ok).toBe(false);
    if (bad.ok) return;
    expect(bad.errors).toEqual([
      'version: expected 1, found 2',
      'extra: unknown field',
      'commands[0].id: missing or of the wrong type',
      'commands[0].more: unknown field',
      'keybindings: expected an array',
      'menus[0].order: missing or of the wrong type',
      'menus[0].accelerator: missing or of the wrong type',
      'pages[0]: expected a string',
      'dialogs.a.kind: missing or of the wrong type',
      'dialogs.a.minSize: missing or of the wrong type',
      'dialogs.a.commands: missing or of the wrong type',
      'dialogs.b: expected an object',
      'notificationKinds: expected an object',
      'messages.a: expected a string',
    ]);
  });

  it('names what drifted, section by section', () => {
    const m = manifest();
    const old = { ...m, pages: ['sd.legacy'], healthServices: [...m.healthServices], dialogs: { ...m.dialogs, 'sd.create': { ...m.dialogs['sd.create']!, commands: ['apps_list'] } } };
    expect(manifestDrift(m, serializeManifest(old as PaddockManifest))).toEqual(['pages: added sd.models; removed sd.legacy', 'dialogs: entries changed']);
    expect(manifestDrift(m, JSON.stringify(m))).toEqual(['formatting differs (regenerate the file)']);
    expect(manifestDrift(m, 'nope')[0]).toMatch(/^checked-in manifest is invalid: not JSON/);
  });
});
