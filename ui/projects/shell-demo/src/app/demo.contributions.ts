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
  type ContributionBatch,
} from '@decryp7/paddock-ui/shell';
import { defineSubmenu, type SubmenuDef } from '@decryp7/paddock-ui/shell/menu';
import type { DemoContext } from './demo-context';

// The registration file of the shell demo: static data and loaders only (plan §9). Its lints run in
// shell/testing/src/reference-apps.spec.ts (contributions) and e2e/shell-demo.spec.ts (lazy imports).
const placeholder = () => import('./placeholder').then((m) => m.Placeholder);

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
      id: 'sd.compact',
      title: msg('sd.compact', 'Compact notices'),
      category: msg('sd.category.demo', 'Demo'),
      checked: (ctx) => ctx.compact(),
      run: (ctx) => ctx.toggleCompact(),
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
  keybindings: [defineKeybinding({ command: 'sd.greet', key: 'Mod+Shift+H' }), defineKeybinding({ command: 'sd.compact', key: 'Mod+Alt+K' })],
  menus: [
    defineMenuItem({ menu: 'help', command: 'sd.greet', group: 'demo', order: 100 }),
    defineMenuItem({ menu: 'help', command: 'sd.reset', group: 'demo', order: 200 }),
    defineMenuItem({ menu: 'view', command: 'sd.compact', group: 'notices', order: 100 }),
    defineMenuItem({ menu: 'file', command: 'sd.job.start', group: 'jobs', order: 100 }),
    defineMenuItem({ menu: 'file', command: 'sd.job.quiet', group: 'jobs', order: 200 }),
    // In the submenu Help > More demos.
    defineMenuItem({ menu: 'sd.more', command: 'sd.fail', group: 'demo', order: 100 }),
    // The context menu of the notices list (pd-context-menu menu="sd.notices"); it may repeat menu-bar commands.
    defineMenuItem({ menu: 'sd.notices', command: 'sd.greet', group: 'demo', order: 100 }),
    defineMenuItem({ menu: 'sd.notices', command: 'sd.reset', group: 'demo', order: 200 }),
  ],
  pages: [definePage({ id: 'sd.home', title: msg('sd.page.home', 'Home'), icon: 'pd:home', order: 100, load: placeholder })],
  statusItems: [defineStatusItem({ id: 'sd.greetings', title: msg('sd.status.greetings', 'Greetings'), side: 'end', priority: 50, load: placeholder })],
  settingsPages: [defineSettingsPage({ id: 'sd.general', group: 'application', title: msg('sd.settings.general', 'General'), order: 100, load: placeholder })],
  dialogs: [
    defineDialog({
      id: 'sd.confirm-reset',
      kind: 'decision',
      title: msg('sd.dialog.confirm-reset', 'Reset greetings?'),
      width: 420,
      commands: [],
      load: placeholder,
    }),
  ],
  healthServices: [defineHealthService({ id: 'sd.engine', label: msg('sd.health.engine', 'the demo engine'), icon: 'pd:engine' })],
  notificationKinds: [defineNotificationKind({ id: 'sd.greeting', title: msg('sd.notification.greeting', 'Greeting'), actions: ['again'] })],
};

/** The demo's submenus (plan S13), registered with `provideSubmenus`. */
export const DEMO_SUBMENUS: readonly SubmenuDef<DemoContext>[] = [
  defineSubmenu<DemoContext>({ id: 'sd.more', menu: 'help', title: msg('sd.menu.more', 'More demos'), group: 'more', order: 900 }),
];
