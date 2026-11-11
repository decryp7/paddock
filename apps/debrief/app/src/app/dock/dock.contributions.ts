import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { PERSPECTIVE_KEYS } from './dock.ids';

// The registration file of the dock's Window commands: stubs only; `dock-commands.ts` and the dock services load when the
// window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:Command category@@dock.commands.category:Window`;

export const DOCK_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'window.popOut', title: $localize`:@@dock.commands.popOut:Pop out to window`, category: CATEGORY, keybinding: 'Ctrl+Alt+O' },
  { id: 'window.float', title: $localize`:@@dock.commands.float:Float panel`, category: CATEGORY },
  { id: 'window.panelMenu', title: $localize`:@@dock.commands.panelMenu:Panel menu (window position)`, category: CATEGORY, keybinding: 'Alt+-' },
  { id: 'window.autoHide', title: $localize`:@@dock.commands.autoHide:Auto hide / pin tool window`, category: CATEGORY, keybinding: 'Ctrl+Alt+H' },
  { id: 'window.autoHideAll', title: $localize`:@@dock.commands.autoHideAll:Auto hide all tool windows`, category: CATEGORY },
  { id: 'window.pinAll', title: $localize`:@@dock.commands.pinAll:Pin all tool windows`, category: CATEGORY },
  { id: 'window.closeAllDocuments', title: $localize`:@@dock.commands.closeAllDocuments:Close all documents`, category: CATEGORY },
  { id: 'window.switcher', title: $localize`:@@dock.commands.switcher:Switch panel (Ctrl+Tab)`, category: CATEGORY },
  { id: 'window.fullScreen', title: $localize`:@@dock.commands.fullScreen:Full screen`, category: CATEGORY, keybinding: 'Shift+Alt+Enter' },
  { id: 'window.fullScreenMode.both', title: $localize`:@@dock.commands.fullScreenMode.both:Full screen does: hide tool windows and chrome, and OS full screen`, category: CATEGORY },
  { id: 'window.fullScreenMode.chrome', title: $localize`:@@dock.commands.fullScreenMode.chrome:Full screen does: hide tool windows and chrome only`, category: CATEGORY },
  { id: 'window.fullScreenMode.os', title: $localize`:@@dock.commands.fullScreenMode.os:Full screen does: OS full screen only`, category: CATEGORY },
  { id: 'window.layout.save', title: $localize`:@@dock.commands.perspective.save:Save current as perspective…`, category: CATEGORY },
  { id: 'window.layout.manage', title: $localize`:@@dock.commands.perspective.manage:Manage perspectives…`, category: CATEGORY },
  // Ctrl+n for each of the first nine perspectives. The title is the default state's (the four built-in names, then the
  // number); once the code is loaded it follows the registry and the saved layouts, as the code's getter does.
  ...[
    $localize`:Perspective name@@dock.perspective.investigate:Investigate`,
    $localize`:Perspective name@@dock.perspective.profiling:Profiling`,
    $localize`:Perspective name@@dock.perspective.live:Live`,
    $localize`:Perspective name@@dock.perspective.diagnostics:Diagnostics`,
  ]
    .concat(Array.from({ length: PERSPECTIVE_KEYS - 4 }, (_, i) => String(i + 5)))
    .map(
      (name, i): CommandStub => ({
        id: `window.perspective.${i + 1}`,
        title: $localize`:Command title|@@dock.commands.perspective:Perspective: ${name}:name:`,
        category: CATEGORY,
        keybinding: `Mod+${i + 1}`,
      }),
    ),
  { id: 'window.layout.reset', title: $localize`:@@dock.commands.perspective.reset:Reset perspective`, category: CATEGORY },
];

export const DOCK_COMMANDS_CONTRIBUTIONS = lazyCommands(() => import('./dock-commands').then((m) => m.DockCommands), DOCK_COMMAND_STUBS);
