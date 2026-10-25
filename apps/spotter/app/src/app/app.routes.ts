import { Routes } from '@angular/router';

export const routes: Routes = [
  { path: '', pathMatch: 'full', title: 'Dashboard · Spotter', loadComponent: () => import('./features/dashboard/dashboard-page').then((m) => m.DashboardPage) },
  { path: 'watches', title: 'Watches · Spotter', loadComponent: () => import('./features/watches/watches-page').then((m) => m.WatchesPage) },
  { path: 'watches/new', title: 'Add watch · Spotter', loadComponent: () => import('./features/editor/editor-page').then((m) => m.EditorPage) },
  { path: 'watches/:id/edit', title: 'Edit watch · Spotter', loadComponent: () => import('./features/editor/editor-page').then((m) => m.EditorPage) },
  { path: 'watches/:id', title: 'Watch · Spotter', loadComponent: () => import('./features/watch-detail/watch-detail-page').then((m) => m.WatchDetailPage) },
  { path: 'settings', title: 'Settings · Spotter', loadComponent: () => import('./features/settings/settings-page').then((m) => m.SettingsPage) },
  { path: 'tray', title: 'Spotter', loadComponent: () => import('./features/tray/tray-page').then((m) => m.TrayPage) },
  { path: '**', redirectTo: '' },
];
