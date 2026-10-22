import type { IconName } from '../shared/icons';

export interface NavItem {
  path: string;
  label: string;
  icon: IconName;
  /** Words the command palette also matches. */
  keywords: string;
  /** Not built yet: the screen is a placeholder, marked "Soon" in the sidebar. */
  planned?: boolean;
}

/** The main window's screens, in sidebar order. */
export const NAV: NavItem[] = [
  { path: '/', label: 'Dashboard', icon: 'dashboard', keywords: 'home overview health live' },
  { path: '/processes', label: 'Processes', icon: 'processes', keywords: 'task manager apps end kill suspend' },
  { path: '/why-slow', planned: true, label: 'Why slow', icon: 'why', keywords: 'diagnosis slow cause anomalies' },
  { path: '/rules', planned: true, label: 'CPU taming', icon: 'rules', keywords: 'rules profiles throttle cap protected' },
  { path: '/cleanup', planned: true, label: 'Cleanup', icon: 'cleanup', keywords: 'clean disk space temp cache quarantine treemap' },
  { path: '/health', planned: true, label: 'Health & tuning', icon: 'health', keywords: 'score tuning startup apps' },
  { path: '/reports', planned: true, label: 'Reports & alerts', icon: 'reports', keywords: 'weekly report alerts history' },
  { path: '/settings', label: 'Settings', icon: 'settings', keywords: 'theme tray plugins about preferences' },
];
