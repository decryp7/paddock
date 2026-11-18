// Playwright's globalSetup for the servers named by `paddockServers` (config.ts): starts them, fails the whole run once
// with one ::error:: line when one never answers, and stops them at the end.
import type { FullConfig } from '@playwright/test';
import { startServers, type ServerSpec } from './server-ready.ts';

export default async function globalSetup(config: FullConfig): Promise<() => Promise<void>> {
  const specs = (config.metadata as { paddockServers?: readonly ServerSpec[] }).paddockServers ?? [];
  try {
    const running = await startServers(specs);
    return () => running.stop();
  } catch (e) {
    console.error((e as Error).message);
    throw e;
  }
}
