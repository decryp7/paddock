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
    path: '',
    component: Shell,
    children: [
      { path: '', pathMatch: 'full', title: 'Dashboard · Dyno', loadComponent: () => import('./features/dashboard/dashboard').then((m) => m.Dashboard) },
      { path: 'processes', title: 'Processes · Dyno', loadComponent: () => import('./features/processes/processes-page').then((m) => m.ProcessesPage) },
      { path: 'inventory', title: 'Inventory · Dyno', loadComponent: () => import('./features/inventory/inventory-page').then((m) => m.InventoryPage) },
      { path: 'benchmarks', title: 'Benchmarks · Dyno', loadComponent: () => import('./features/benchmarks/bench-page').then((m) => m.BenchPage) },
      { path: 'cpu', title: 'CPU · Dyno', loadComponent: () => import('./features/cpu/cpu-detail').then((m) => m.CpuDetail) },
      { path: 'why-slow', title: 'Why slow · Dyno', loadComponent: () => import('./features/why-slow/why-slow-page').then((m) => m.WhySlowPage) },
      { path: 'rules', title: 'CPU taming · Dyno', loadComponent: () => import('./features/rules/rules-page').then((m) => m.RulesPage) },
      { path: 'cleanup', title: 'Cleanup · Dyno', loadComponent: () => import('./features/cleanup/cleanup-page').then((m) => m.CleanupPage) },
      { path: 'health', title: 'Health & tuning · Dyno', loadComponent: () => import('./features/health/health-page').then((m) => m.HealthPage) },
      { path: 'reports', title: 'Reports & alerts · Dyno', loadComponent: () => import('./features/reports/reports-page').then((m) => m.ReportsPage) },
      { path: 'settings', title: 'Settings · Dyno', loadComponent: () => import('./features/settings/settings-page').then((m) => m.SettingsPage) },
      { path: '**', redirectTo: '' },
    ],
  },
];
