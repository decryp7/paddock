import type { WindowPlatform } from '../core/platform';
import type { Priority } from '../core/api.types';

/** The words each OS uses for the things Dyno names, so a Mac reads "Activity Monitor" and "Low Power Mode", not Windows' words. */
export interface OsTerms {
  /** The OS's own process viewer. */
  readonly taskManager: string;
  /** What a process holds open: handles on Windows, file descriptors elsewhere. */
  readonly handles: string;
  /** The OS's reduced-power state. */
  readonly lowPower: string;
  /** An example of "a change you are about to make", for the benchmark label. */
  readonly powerExample: string;
  /** Whether the OS has a per-process efficiency mode and CPU affinity. */
  readonly hasEfficiencyMode: boolean;
}

const TERMS: Record<WindowPlatform, OsTerms> = {
  windows: { taskManager: 'Task Manager', handles: 'Handles', lowPower: 'Battery saver', powerExample: 'Power mode: High performance', hasEfficiencyMode: true },
  macos: { taskManager: 'Activity Monitor', handles: 'File descriptors', lowPower: 'Low Power Mode', powerExample: 'Low Power Mode: On', hasEfficiencyMode: false },
  linux: { taskManager: 'System Monitor', handles: 'File descriptors', lowPower: 'Power saver', powerExample: 'Power profile: Performance', hasEfficiencyMode: false },
  browser: { taskManager: 'the task manager', handles: 'Handles', lowPower: 'Battery saver', powerExample: 'Power mode: High performance', hasEfficiencyMode: true },
};

/** The terms of a platform. */
export function osTerms(platform: WindowPlatform): OsTerms {
  return TERMS[platform];
}

const NICE: Record<Priority, number> = { idle: 19, belowNormal: 10, normal: 0, aboveNormal: -5, high: -10 };

/** A priority class as the Unix nice value it stands for (`0 (normal)`), for macOS and Linux. */
export function niceText(priority: Priority, label: string): string {
  return `${NICE[priority]} (${label.toLowerCase()})`;
}
