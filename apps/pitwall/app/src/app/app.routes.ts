import type { Routes } from '@angular/router';

/** Every screen is lazy-loaded so the first paint stays small (ECharts ships with the dashboard and charts only). */
export const routes: Routes = [
  { path: '', pathMatch: 'full', title: 'Dashboard · Pitwall', loadComponent: () => import('./features/dashboard/dashboard').then((m) => m.Dashboard) },
  { path: 'models', title: 'Models · Pitwall', loadComponent: () => import('./features/models/models-page').then((m) => m.ModelsPage) },
  { path: 'engines', title: 'Engines · Pitwall', loadComponent: () => import('./features/engines/engines-page').then((m) => m.EnginesPage) },
  { path: 'apps', title: 'Apps & keys · Pitwall', loadComponent: () => import('./features/apps/apps-page').then((m) => m.AppsPage) },
  { path: 'requests', title: 'Requests · Pitwall', loadComponent: () => import('./features/requests/requests-page').then((m) => m.RequestsPage) },
  { path: 'playground', title: 'Playground · Pitwall', loadComponent: () => import('./features/playground/playground-page').then((m) => m.PlaygroundPage) },
  { path: 'benchmarks', title: 'Benchmarks · Pitwall', loadComponent: () => import('./features/benchmarks/benchmarks-page').then((m) => m.BenchmarksPage) },
  { path: 'plugins', title: 'Plugins & tools · Pitwall', loadComponent: () => import('./features/plugins/plugins-page').then((m) => m.PluginsPage) },
  { path: 'settings', title: 'Settings · Pitwall', loadComponent: () => import('./features/settings/settings-page').then((m) => m.SettingsPage) },
  { path: 'tray', title: 'Pitwall', loadComponent: () => import('./features/tray/tray-popover').then((m) => m.TrayPopover) },
  { path: '**', redirectTo: '' },
];
