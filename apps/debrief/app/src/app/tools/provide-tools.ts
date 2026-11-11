import { WhatsNewService } from '../whats-new/whats-new.service';
import { EnvironmentProviders, inject, makeEnvironmentProviders, provideAppInitializer } from '@angular/core';
import { DROPPED_FILES_HANDLERS } from '../core/dropped-files';
import { provideToolsBackend } from '../core/tools-backend';
import { CoreUiState } from '../lifecycle/core-ui-state';
import { LifecycleService } from '../lifecycle/lifecycle.service';
import { SessionService } from '../lifecycle/session.service';
import { UI_STATE_CONTRIBUTIONS } from '../lifecycle/ui-state';
import { OpenRequests } from '../projects/open-requests';
import { ProjectUiState } from '../projects/project-ui-state';
import { pluginPackageDropHandler } from '../plugins-panel/package-drop';
import { ToolCatalog } from './tool-catalog.store';
import { toolsActivityItems } from './tools-activity';
import { toolsViews } from './tools-views';
import { TOOLS_COMMANDS_CONTRIBUTIONS } from './tools.contributions';
import { contributeFrom } from '../extensions/contributions';

/**
 * Registers the tools (Performance Monitor, process monitor, diagnostics collector, plugin
 * health), app lifecycle (administrator badge, safe mode and crash banners, session autosave),
 * projects and saved queries. One line in `app.config.ts`.
 */
export function provideTools(): EnvironmentProviders {
  return makeEnvironmentProviders([
    ...provideToolsBackend(),
    ...contributeFrom('views', () => toolsViews(inject(ToolCatalog))),
    // Stubs only: the commands' code loads when the window is idle or one runs.
    ...TOOLS_COMMANDS_CONTRIBUTIONS,
    ...contributeFrom('activityItems', toolsActivityItems),
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => inject(CoreUiState).contributions },
    { provide: UI_STATE_CONTRIBUTIONS, multi: true, useFactory: () => inject(ProjectUiState).contributions },
    // `.debriefplugin` files dropped on the window are reviewed for installing, never opened as logs.
    { provide: DROPPED_FILES_HANDLERS, multi: true, useFactory: pluginPackageDropHandler },
    provideAppInitializer(() => startTools()),
  ]);
}

/** Starts the background parts. Not awaited: a slow backend must not delay the first paint. */
export function startTools(): void {
  const life = inject(LifecycleService);
  const session = inject(SessionService);
  const requests = inject(OpenRequests);
  // Reads which tools the engine offers (and keeps it current); views and commands follow.
  inject(ToolCatalog);
  const whatsNew = inject(WhatsNewService);
  void life.init().then(() => {
    void whatsNew.checkOnStart();
    session.start();
    return requests.start();
  });
}
