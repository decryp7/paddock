import { readFileSync } from 'node:fs';
import { buildMenus, MenuState, MenuView } from './menu-model';

/**
 * The menu bar pinned for four states (golden files in `golden/`): every command the menus place is registered, so each
 * placement, label, check mark, separator and submenu shows. M1 moved the command placements into `menus`
 * contributions; these files were written by the model before that move and must not change with it. A deliberate
 * change to the menus rewrites them (`ng test -u`) and the diff is reviewed.
 */
const MENU_SOURCES = ['src/app/shell/menu/menu-model.ts', 'src/app/shell/menu/menu.contributions.ts'];

/** Every command id the menus can place, from their sources (plus the numbered perspectives). */
function placedIds(): string[] {
  const ids = new Set<string>(Array.from({ length: 9 }, (_, i) => `window.perspective.${i + 1}`));
  for (const path of MENU_SOURCES) {
    let text: string;
    try {
      text = readFileSync(path, 'utf8');
    } catch {
      continue;
    }
    for (const m of text.matchAll(/(?:cmd|appMenu|item)\(\s*'([a-zA-Z0-9.]+)'/g)) ids.add(m[1]!);
    for (const m of text.matchAll(/command:\s*'([a-zA-Z0-9.]+)'/g)) ids.add(m[1]!);
  }
  return [...ids].sort();
}

const view = (id: string, title: string, category?: MenuView['category'], patch: Partial<MenuView> = {}): MenuView => ({ id, title, category, open: false, ...patch });

const VIEWS: MenuView[] = [
  view('timeline', 'Timeline', 'core', { open: true }),
  view('events', 'Events', 'core', { open: true }),
  view('inspector', 'Inspector', 'core'),
  view('profile', 'Profile', 'core'),
  view('extra-core', 'Extra core', 'core'),
  view('plugins', 'Plugins', 'Plugins'),
  view('moment', 'Moment', 'Analysis'),
  view('markers', 'Markers', 'Analysis', { open: true }),
  view('live-alerts', 'Alerts', 'Live'),
  view('profiling.captures', 'Captures', 'Profiling'),
  view('profiling.inverted', 'Inverted tree', 'Profiling', { menu: 'palette' }),
  view('perfmon', 'Performance Monitor', 'Tools'),
  view('report', 'Report', 'Sharing'),
  view('error-snapshots', 'Error snapshots', 'Diagnostics'),
  view('ai-chat', 'AI assistant', 'AI'),
  view('registry', 'Registry', undefined, { plugin: 'Windows registry' }),
  view('evtx', 'Event log', undefined, { plugin: 'Windows events' }),
];

function state(patch: Partial<MenuState>): MenuState {
  const ids = placedIds();
  return {
    commands: new Map(ids.map((id) => [id, { id, title: `T ${id}`, ...(id.length % 4 === 0 ? { keybinding: `Mod+${id.length}` } : {}) }])),
    views: VIEWS,
    sidebarOpen: true,
    theme: 'console',
    scheme: 'dark',
    sourceColors: 'source',
    activePerspective: 1,
    timezone: 'utc',
    markedOnly: false,
    undoLabel: 'level filter change',
    redoLabel: null,
    windows: [{ label: 'main', title: 'Debrief', current: true }],
    popouts: [],
    recent: { projects: [{ path: '/p/a.debrief', name: 'a' }], files: ['/logs/web01/app.log', '/logs/web02/app.log'] },
    nativeAppMenu: false,
    ...patch,
  };
}

describe('menus (golden)', () => {
  it('places every command it did before M1', () => {
    expect(placedIds().length).toBeGreaterThan(100);
  });

  it('in-app menu bar, everything registered', async () => {
    await expect(JSON.stringify(buildMenus(state({})), null, 1)).toMatchFileSnapshot('golden/menus-in-app.json');
  });

  it('macOS native menu bar', async () => {
    await expect(JSON.stringify(buildMenus(state({ nativeAppMenu: true })), null, 1)).toMatchFileSnapshot('golden/menus-native.json');
  });

  it('other states: follow system, nothing recent, toggles on, several windows and pop-outs', async () => {
    const s = state({
      theme: null,
      scheme: 'system',
      sourceColors: 'instance',
      activePerspective: 3,
      timezone: 'local',
      markedOnly: true,
      sidebarOpen: false,
      undoLabel: null,
      redoLabel: 'clear',
      recent: { projects: [], files: [] },
      windows: [
        { label: 'main', title: 'Debrief', current: false },
        { label: 'w2', title: '', current: true },
      ],
      popouts: [{ id: 'markers', title: 'Markers' }],
    });
    await expect(JSON.stringify(buildMenus(s), null, 1)).toMatchFileSnapshot('golden/menus-other.json');
  });

  it('nothing registered: only the items that are not commands', async () => {
    await expect(JSON.stringify(buildMenus(state({ commands: new Map(), views: [], recent: undefined })), null, 1)).toMatchFileSnapshot('golden/menus-empty.json');
  });
});
