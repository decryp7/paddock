import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { DebriefCommand, DebriefView } from '../extensions/tokens';
import { provideConfigDiffBackend } from './config-diff-backend';
import { debriefContributionProviders } from '../extensions/contributions';

/** The configuration diff panel. Same extension point as plugin views. */
export const CONFIG_DIFF_VIEWS: DebriefView[] = [
  { id: 'config-diff', category: 'Tools', title: $localize`:@@configDiff.provide.view.title:Configuration diff`, icon: 'columns', defaultPosition: 'center', layout: { area: 'document', min: { width: 420, height: 240 }, pref: { width: 640, height: 400 }, priority: 55 }, component: () => import('./config-diff-view').then((m) => m.ConfigDiffView) },
];

export const CONFIG_DIFF_COMMANDS: DebriefCommand[] = [
  { id: 'tools.configDiff.show', title: $localize`:@@configDiff.provide.command.compare:Compare configuration files…`, category: $localize`:@@configDiff.provide.command.category:Tools`, view: 'config-diff', run: (ctx) => ctx.showView('config-diff') },
];

/** Registers the configuration diff (view, command, backend). One line in `app.config.ts`. */
export function provideConfigDiff(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...provideConfigDiffBackend(),
    ...debriefContributionProviders({ views: CONFIG_DIFF_VIEWS }),
    ...debriefContributionProviders({ commands: CONFIG_DIFF_COMMANDS }),
  ]);
}
