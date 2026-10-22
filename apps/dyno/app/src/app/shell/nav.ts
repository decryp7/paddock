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
  { path: '/why-slow', label: 'Why slow', icon: 'why', keywords: 'diagnosis slow cause evidence lag freeze cpu memory disk heat' },
  { path: '/inventory', label: 'Inventory', icon: 'memory', keywords: 'memory ram modules slots upgrade hardware dimm' },
  { path: '/network', label: 'Network', icon: 'network', keywords: 'ports listening connections sockets firewall exposed ip remote host' },
  { path: '/benchmarks', label: 'Benchmarks', icon: 'gauge', keywords: 'benchmark speed score cpu memory disk before after compare' },
  { path: '/rules', label: 'CPU taming', icon: 'rules', keywords: 'rules profiles throttle cap protected gaming meeting battery' },
  { path: '/alarms', label: 'Alarms', icon: 'bell', keywords: 'alerts notifications acknowledge shelve mute thresholds custom rules' },
  { path: '/reports', label: 'Reports', icon: 'reports', keywords: 'weekly report summary week trend export markdown html' },
  { path: '/recorder', label: 'Flight recorder', icon: 'camera', keywords: 'timeline capture snapshot last ten minutes' },
  { path: '/cleanup', label: 'Cleanup', icon: 'cleanup', keywords: 'temp caches disk space free up quarantine trash recycle bin developer build output' },
  { path: '/health', label: 'Health', icon: 'health', keywords: 'score reasons startup apps sign-in' },
  { path: '/tuning', label: 'Tuning', icon: 'sparkle', keywords: 'power plan sleep hibernation battery charge limit indexing visual effects background apps suggestions undo' },
  { path: '/diagnostics', label: 'Diagnostics', icon: 'file', keywords: 'bundle zip bug report redacted support' },
  { path: '/settings', label: 'Settings', icon: 'settings', keywords: 'theme tray plugins about preferences' },
];

/** What is on the roadmap but has no screen: listed in one dialog, never as a route or a page of its own. */
export const COMING: { label: string; icon: IconName; text: string }[] = [
  { label: 'Administrator tuning', icon: 'info', text: 'The helper that lets Dyno apply the tuning items that need administrator rights (Defender exclusions, hibernation, charge limit), always after a prompt and with an undo.' },
  { label: 'Issues', icon: 'info', text: 'Crashes, hangs and failing services from the OS logs, with a daily stability score.' },
];
