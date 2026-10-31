import type { Type } from '@angular/core';

export interface NavItem {
  /** The page's id after `pw.` and its route path (`models` is `#/models`). */
  id: string;
  label: string;
  /** One of Pitwall's icons (`pw:<icon>`). */
  icon: string;
  /** Loads the screen's component on first visit. */
  load: () => Promise<Type<unknown>>;
}

/**
 * The main window's screens, in sidebar order. Settings is the shell's Settings window (the app menu, Mod+, and the
 * sidebar's gear), not a page.
 */
export const NAV: readonly NavItem[] = [
  { id: 'dashboard', label: 'Dashboard', icon: 'dashboard', load: () => import('../features/dashboard/dashboard').then((m) => m.Dashboard) },
  { id: 'models', label: 'Models', icon: 'models', load: () => import('../features/models/models-page').then((m) => m.ModelsPage) },
  { id: 'engines', label: 'llama.cpp', icon: 'engines', load: () => import('../features/engines/engines-page').then((m) => m.EnginesPage) },
  { id: 'apps', label: 'Apps & keys', icon: 'apps', load: () => import('../features/apps/apps-page').then((m) => m.AppsPage) },
  { id: 'requests', label: 'Requests', icon: 'requests', load: () => import('../features/requests/requests-page').then((m) => m.RequestsPage) },
  { id: 'playground', label: 'Playground', icon: 'playground', load: () => import('../features/playground/playground-page').then((m) => m.PlaygroundPage) },
  { id: 'benchmarks', label: 'Benchmarks', icon: 'benchmarks', load: () => import('../features/benchmarks/benchmarks-page').then((m) => m.BenchmarksPage) },
];
