import { EnvironmentProviders, inject, makeEnvironmentProviders, provideAppInitializer } from '@angular/core';
import { WorkspaceStore } from '../core/workspace.store';
import { DebriefView } from '../extensions/tokens';
import { EVENT_ID_FORMATTER } from '../extensions/field-formatters';
import { builtinActivityItems } from './builtin-activity';
import { BuiltinCommands } from './builtin-commands';
import { BUILTIN_VIEWS } from './builtin-views';
import { RegionNavigator } from '../shell/regions';
import { EditCommands } from '../shell/edit-commands';
import { PasteCommands } from '../sources/paste/paste-commands';
import { startTextDrops } from '../sources/paste/paste-drop';
import { contributeFrom, debriefContributionProviders } from '../extensions/contributions';

/** Registers the built-in views, commands and formatters through the same extension points plugins use. */
/** `views` can be replaced in tests (e.g. to stub heavy panels). */
export function provideDebriefBuiltins(views: readonly DebriefView[] = BUILTIN_VIEWS): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...debriefContributionProviders({ views: views }),
    ...contributeFrom('commands', () => inject(BuiltinCommands).commands),
    ...contributeFrom('commands', () => inject(RegionNavigator).commands),
    ...contributeFrom('commands', () => inject(EditCommands).commands),
    ...contributeFrom('commands', () => inject(PasteCommands).commands),
    provideAppInitializer(startTextDrops),
    ...debriefContributionProviders({ fieldFormatters: [EVENT_ID_FORMATTER] }),
    ...contributeFrom('activityItems', () => builtinActivityItems(inject(WorkspaceStore))),
  ]);
}
