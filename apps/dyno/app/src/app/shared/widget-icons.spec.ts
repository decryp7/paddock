import { STYLE_SUPPORT_GOLDEN } from '../core/generated/style-support-golden';
import { MockWidgets } from '../core/mock/widgets';
import { WIDGET_ICONS } from './widget-icons';

/** The icon names the engine's catalogues use (`icon:` in dyno-core/src/widgets/{popover,dashboard,menu_bar}.rs). */
const ENGINE_ICONS = ['alarm', 'disk', 'cpu', 'network', 'issue', 'counters', 'apps', 'graph', 'number', 'bars', 'ring', 'heatmap', 'health', 'diagnosis', 'events', 'history', 'consumers', 'meter'];

describe('widget icons', () => {
  it('draws every icon name the engine uses, as a 24 px outline in currentColor', () => {
    for (const name of ENGINE_ICONS) {
      expect(WIDGET_ICONS[name], name).toContain('viewBox="0 0 24 24"');
      expect(WIDGET_ICONS[name], name).toContain('stroke="currentColor"');
    }
  });

  it('has a glyph for every icon of the catalogues the mock mirrors', () => {
    const w = new MockWidgets();
    for (const surface of ['popover', 'dashboard', 'menuBar'] as const) {
      const view = Object.values(w.view(surface))[0] as { catalogue: { kinds: { icon: string }[] } };
      for (const k of view.catalogue.kinds) expect(WIDGET_ICONS[k.icon], k.icon).toBeDefined();
    }
  });

  it("has a glyph for every icon the engine gives a menu-bar row of one picture (the engine's own values)", () => {
    for (const os of ['macos', 'windows', 'linux'] as const) for (const f of STYLE_SUPPORT_GOLDEN[os].fixed) expect(WIDGET_ICONS[f.icon], f.icon).toBeDefined();
  });
});
