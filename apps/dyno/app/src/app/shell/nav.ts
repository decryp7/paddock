import type { Type } from '@angular/core';
import type { IconName } from '../shared/icons';

export interface NavItem {
  path: string;
  label: string;
  icon: IconName;
  /** Loads the screen's component on first visit. */
  load: () => Promise<Type<unknown>>;
}

/** The main window's screens, in sidebar order (Settings is the shell's window, opened from the menu, the gear and Mod+,). */
export const NAV: NavItem[] = [
  { path: '/', label: 'Dashboard', icon: 'dashboard', load: () => import('../features/dashboard/dashboard').then((m) => m.Dashboard) },
  { path: '/processes', label: 'Processes', icon: 'processes', load: () => import('../features/processes/processes-page').then((m) => m.ProcessesPage) },
  { path: '/why-slow', label: 'Why slow', icon: 'why', load: () => import('../features/why-slow/why-slow-page').then((m) => m.WhySlowPage) },
  { path: '/throttling', label: 'Throttling', icon: 'temperature', load: () => import('../features/throttling/throttling-page').then((m) => m.ThrottlingPage) },
  { path: '/issues', label: 'Issues', icon: 'alert', load: () => import('../features/issues/issues-page').then((m) => m.IssuesPage) },
  { path: '/inventory', label: 'Inventory', icon: 'memory', load: () => import('../features/inventory/inventory-page').then((m) => m.InventoryPage) },
  { path: '/disk-map', label: 'Disk map', icon: 'disk', load: () => import('../features/disk-map/disk-map-page').then((m) => m.DiskMapPage) },
  { path: '/network', label: 'Network', icon: 'network', load: () => import('../features/network/network-page').then((m) => m.NetworkPage) },
  { path: '/map', label: 'Connections map', icon: 'globe', load: () => import('../features/map/map-page').then((m) => m.MapPage) },
  { path: '/lists', label: 'IP lists', icon: 'shield', load: () => import('../features/lists/lists-page').then((m) => m.ListsPage) },
  { path: '/firewall', label: 'Firewall', icon: 'lock', load: () => import('../features/firewall/firewall-page').then((m) => m.FirewallPage) },
  { path: '/counters', label: 'Counters', icon: 'trend', load: () => import('../features/counters/counters-page').then((m) => m.CountersPage) },
  { path: '/benchmarks', label: 'Benchmarks', icon: 'gauge', load: () => import('../features/benchmarks/bench-page').then((m) => m.BenchPage) },
  { path: '/alarms', label: 'Alarms', icon: 'bell', load: () => import('../features/alarms/alarms-page').then((m) => m.AlarmsPage) },
  { path: '/reports', label: 'Reports', icon: 'reports', load: () => import('../features/reports/reports-page').then((m) => m.ReportsPage) },
  { path: '/recorder', label: 'Flight recorder', icon: 'camera', load: () => import('../features/recorder/recorder-page').then((m) => m.RecorderPage) },
  { path: '/health', label: 'Health', icon: 'health', load: () => import('../features/health/health-page').then((m) => m.HealthPage) },
  { path: '/backup', label: 'Backup health', icon: 'restore', load: () => import('../features/backup/backup-page').then((m) => m.BackupPage) },
  { path: '/power', label: 'Power', icon: 'power', load: () => import('../features/power/power-page').then((m) => m.PowerPage) },
  { path: '/extras', label: 'Extras', icon: 'layers', load: () => import('../features/extras/extras-page').then((m) => m.ExtrasPage) },
  { path: '/lifespan', label: 'Lifespan', icon: 'battery', load: () => import('../features/lifespan/lifespan-page').then((m) => m.LifespanPage) },
  { path: '/diagnostics', label: 'Diagnostics', icon: 'file', load: () => import('../features/diagnostics/diagnostics-page').then((m) => m.DiagnosticsPage) },
];
