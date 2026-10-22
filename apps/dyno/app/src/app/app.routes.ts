import type { Routes } from '@angular/router';
import { Shell } from './shell/shell';

/** The main window's screens (lazy-loaded, so the dashboard stays fast) and the tray dropdown window (`#/tray`). */
export const routes: Routes = [
  {
    path: 'tray',
    title: 'Dyno',
    loadComponent: () => import('./features/tray/tray').then((m) => m.Tray),
  },
  {
    // Each menu-bar item opens its own popover window (`#/popover/<item id>`); every one shows the shared compact dashboard.
    path: 'popover/:id',
    title: 'Dyno',
    loadComponent: () => import('./features/tray/tray').then((m) => m.Tray),
  },
  {
    path: '',
    component: Shell,
    children: [
      { path: '', pathMatch: 'full', title: 'Dashboard · Dyno', loadComponent: () => import('./features/dashboard/dashboard').then((m) => m.Dashboard) },
      { path: 'processes', title: 'Processes · Dyno', loadComponent: () => import('./features/processes/processes-page').then((m) => m.ProcessesPage) },
      { path: 'inventory', title: 'Inventory · Dyno', loadComponent: () => import('./features/inventory/inventory-page').then((m) => m.InventoryPage) },
      { path: 'benchmarks', title: 'Benchmarks · Dyno', loadComponent: () => import('./features/benchmarks/bench-page').then((m) => m.BenchPage) },
      { path: 'network', title: 'Network · Dyno', loadComponent: () => import('./features/network/network-page').then((m) => m.NetworkPage) },
      { path: 'cpu', title: 'CPU · Dyno', loadComponent: () => import('./features/cpu/cpu-detail').then((m) => m.CpuDetail) },
      { path: 'rules', title: 'CPU taming · Dyno', loadComponent: () => import('./features/rules/rules-page').then((m) => m.RulesPage) },
      { path: 'alarms', title: 'Alarms · Dyno', loadComponent: () => import('./features/alarms/alarms-page').then((m) => m.AlarmsPage) },
      { path: 'recorder', title: 'Flight recorder · Dyno', loadComponent: () => import('./features/recorder/recorder-page').then((m) => m.RecorderPage) },
      { path: 'diagnostics', title: 'Diagnostics · Dyno', loadComponent: () => import('./features/diagnostics/diagnostics-page').then((m) => m.DiagnosticsPage) },
      { path: 'health', title: 'Health & tuning · Dyno', loadComponent: () => import('./features/health/health-page').then((m) => m.HealthPage) },
      { path: 'settings', title: 'Settings · Dyno', loadComponent: () => import('./features/settings/settings-page').then((m) => m.SettingsPage) },
      { path: '**', redirectTo: '' },
    ],
  },
];
