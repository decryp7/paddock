import type { Routes } from '@angular/router';

/** The screens, each loaded on first visit. */
export const routes: Routes = [
  { path: '', pathMatch: 'full', title: 'Dashboard · Pitcrew', loadComponent: () => import('./features/dashboard/dashboard-page').then((m) => m.DashboardPage) },
  { path: 'runners', title: 'Runners · Pitcrew', loadComponent: () => import('./features/runners/runners-page').then((m) => m.RunnersPage) },
  { path: 'runners/:name', title: 'Runner · Pitcrew', loadComponent: () => import('./features/runners/runner-detail-page').then((m) => m.RunnerDetailPage) },
  { path: 'activity', title: 'Activity · Pitcrew', loadComponent: () => import('./features/activity/activity-page').then((m) => m.ActivityPage) },
  { path: 'insights', title: 'Insights · Pitcrew', loadComponent: () => import('./features/insights/insights-page').then((m) => m.InsightsPage) },
  { path: 'storage', title: 'Storage · Pitcrew', loadComponent: () => import('./features/storage/storage-page').then((m) => m.StoragePage) },
  { path: 'logs', title: 'Logs · Pitcrew', loadComponent: () => import('./features/logs/logs-page').then((m) => m.LogsPage) },
  { path: 'logs/:runner', title: 'Logs · Pitcrew', loadComponent: () => import('./features/logs/logs-page').then((m) => m.LogsPage) },
  { path: 'updates', title: 'Updates · Pitcrew', loadComponent: () => import('./features/updates/updates-page').then((m) => m.UpdatesPage) },
  { path: 'settings', title: 'Settings · Pitcrew', loadComponent: () => import('./features/settings/settings-page').then((m) => m.SettingsPage) },
  { path: '**', redirectTo: '' },
];
