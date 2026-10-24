import {
  defineCommand,
  defineDialog,
  defineHealthService,
  defineKeybinding,
  defineMenuItem,
  defineNotificationKind,
  definePage,
  defineSettingsPage,
  defineStatusItem,
  msg,
  type,
  type ContributionBatch,
} from '@decryp7/paddock-ui/shell';
import type { DemoContext } from './demo-context';
import type { DemoStatus } from './dialogs/demo-ipc';

// The registration file of the shell demo: static data and loaders only (plan §9). Its lints run in
// shell/testing/src/reference-apps.spec.ts (contributions) and e2e/shell-demo.spec.ts (lazy imports).
const placeholder = () => import('./placeholder').then((m) => m.Placeholder);

/** A decision: modal, fitted to its content. */
export const CONFIRM_RESET_DIALOG = defineDialog({
  id: 'sd.confirm-reset',
  kind: 'decision',
  title: msg('sd.dialog.confirm-reset', 'Reset greetings?'),
  width: 420,
  result: type<boolean>(),
  commands: [],
  load: () => import('./dialogs/confirm-reset-dialog').then((m) => m.ConfirmResetDialog),
});

/** A form that blocks its opener. */
export const RENAME_DIALOG = defineDialog({
  id: 'sd.rename',
  kind: 'form',
  modality: 'modal',
  title: msg('sd.dialog.rename', 'Rename'),
  width: 440,
  result: type<string>(),
  commands: [],
  load: () => import('./dialogs/rename-dialog').then((m) => m.RenameDialog),
});

/** A viewer: modeless, 680x560 at least, grows only. */
export const ABOUT_DIALOG = defineDialog({
  id: 'sd.about',
  kind: 'viewer',
  title: msg('sd.dialog.about', 'About the shell demo'),
  commands: [],
  load: () => import('./dialogs/about-dialog').then((m) => m.AboutDialog),
});

/** A tool: modeless, large, remembers its size; it calls the engine, so its contract names that command. */
export const INSPECTOR_DIALOG = defineDialog({
  id: 'sd.inspector',
  kind: 'tool',
  title: msg('sd.dialog.inspector', 'Engine inspector'),
  width: 720,
  height: 480,
  result: type<DemoStatus>(),
  commands: ['demo_status'],
  providers: () => import('./dialogs/demo-ipc').then((m) => m.INSPECTOR_PROVIDERS),
  load: () => import('./dialogs/inspector-dialog').then((m) => m.InspectorDialog),
});

/** A required choice: a modal without Cancel. */
export const CHOOSE_STYLE_DIALOG = defineDialog({
  id: 'sd.choose-style',
  kind: 'decision',
  title: msg('sd.dialog.choose-style', 'Choose a greeting style'),
  width: 400,
  requiredChoice: true,
  result: type<'casual' | 'formal'>(),
  commands: [],
  load: () => import('./dialogs/choose-style-dialog').then((m) => m.ChooseStyleDialog),
});

export const DEMO_DIALOGS = [CONFIRM_RESET_DIALOG, RENAME_DIALOG, ABOUT_DIALOG, INSPECTOR_DIALOG, CHOOSE_STYLE_DIALOG] as const;

export const DEMO_CONTRIBUTIONS: ContributionBatch<DemoContext> = {
  commands: [
    defineCommand<DemoContext>({
      id: 'sd.greet',
      title: msg('sd.greet', 'Say hello'),
      category: msg('sd.category.demo', 'Demo'),
      run: (ctx) => {
        const n = ctx.greet();
        ctx.notify({ level: 'info', kind: 'sd.greeting', message: `Hello from the shell demo (${n})` });
      },
    }),
    defineCommand<DemoContext>({
      id: 'sd.reset',
      title: msg('sd.reset', 'Reset greetings'),
      category: msg('sd.category.demo', 'Demo'),
      enabled: (ctx) => ctx.greetings() > 0,
      run: (ctx) => {
        ctx.reset();
        ctx.notify({ level: 'success', message: 'Greetings reset' });
      },
    }),
    defineCommand<DemoContext>({
      id: 'sd.job.start',
      title: msg('sd.job.start', 'Start sample job'),
      category: msg('sd.category.jobs', 'Jobs'),
      run: (ctx) => void ctx.startJob(),
    }),
    defineCommand<DemoContext>({
      id: 'sd.job.quiet',
      title: msg('sd.job.quiet', 'Start quiet job'),
      category: msg('sd.category.jobs', 'Jobs'),
      run: (ctx) => void ctx.startQuietJob(),
    }),
    defineCommand<DemoContext>({
      id: 'sd.fail',
      title: msg('sd.fail', 'Fail on purpose'),
      category: msg('sd.category.demo', 'Demo'),
      run: () => {
        throw new Error('this command always fails');
      },
    }),
  ],
  keybindings: [defineKeybinding({ command: 'sd.greet', key: 'Mod+Shift+H' })],
  menus: [
    defineMenuItem({ menu: 'help', command: 'sd.greet', group: 'demo', order: 100 }),
    defineMenuItem({ menu: 'help', command: 'sd.reset', group: 'demo', order: 200 }),
  ],
  pages: [definePage({ id: 'sd.home', title: msg('sd.page.home', 'Home'), icon: 'pd:home', order: 100, load: placeholder })],
  statusItems: [defineStatusItem({ id: 'sd.greetings', title: msg('sd.status.greetings', 'Greetings'), side: 'end', priority: 50, load: placeholder })],
  settingsPages: [defineSettingsPage({ id: 'sd.general', group: 'application', title: msg('sd.settings.general', 'General'), order: 100, load: placeholder })],
  // One dialog of each kind (ADR 0009), plus a modal form and a required choice. Contracts: dialog-contracts.json.
  dialogs: DEMO_DIALOGS,
  healthServices: [defineHealthService({ id: 'sd.engine', label: msg('sd.health.engine', 'the demo engine'), icon: 'pd:engine' })],
  notificationKinds: [defineNotificationKind({ id: 'sd.greeting', title: msg('sd.notification.greeting', 'Greeting'), actions: ['again'] })],
};
