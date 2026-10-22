import type { IconName } from '../shared/icons';

export interface NavItem {
  path: string;
  label: string;
  icon: IconName;
  /** Words the command palette also matches. */
  keywords: string;
}

/** The main window's screens, in sidebar order. */
export const NAV: NavItem[] = [
  { path: '/', label: 'Dashboard', icon: 'dashboard', keywords: 'home overview health live' },
  { path: '/processes', label: 'Processes', icon: 'processes', keywords: 'task manager apps end kill suspend' },
  { path: '/inventory', label: 'Inventory', icon: 'memory', keywords: 'memory ram modules slots upgrade hardware dimm' },
  { path: '/network', label: 'Network', icon: 'network', keywords: 'ports listening connections sockets firewall exposed ip remote host' },
  { path: '/benchmarks', label: 'Benchmarks', icon: 'gauge', keywords: 'benchmark speed score cpu memory disk before after compare' },
  { path: '/rules', label: 'CPU taming', icon: 'rules', keywords: 'rules profiles throttle cap protected gaming meeting battery' },
  { path: '/alarms', label: 'Alarms', icon: 'bell', keywords: 'alerts notifications acknowledge shelve mute thresholds custom rules' },
  { path: '/recorder', label: 'Flight recorder', icon: 'camera', keywords: 'timeline capture snapshot last ten minutes' },
  { path: '/health', label: 'Health & tuning', icon: 'health', keywords: 'score tuning startup apps' },
  { path: '/diagnostics', label: 'Diagnostics', icon: 'file', keywords: 'bundle zip bug report redacted support' },
  { path: '/settings', label: 'Settings', icon: 'settings', keywords: 'theme tray plugins about preferences' },
];

/** What is on the roadmap but has no screen: listed in one dialog, never as a route or a page of its own. */
export const COMING: { label: string; icon: IconName; text: string }[] = [
  { label: 'Why slow', icon: 'why', text: 'A full page for the diagnosis: the likeliest cause, the evidence, memory leaks, periodic spikes and restart loops.' },
  { label: 'Cleanup', icon: 'cleanup', text: 'Previews, then removes temp files, caches and developer leftovers to the Recycle Bin or Trash, with a disk space map.' },
  { label: 'Weekly report', icon: 'reports', text: 'A weekly health summary with the alarms of the week and what changed.' },
];
