import { EnvironmentProviders, makeEnvironmentProviders } from '@angular/core';
import { contributionProviders, provideContributionKinds, provideContributions, type ContributionBatch } from '@decryp7/paddock-ui/shell';
import type {
  ActivityItemDef,
  AssistantContextDef,
  ExplorerNode,
  ExplorerProviderDef,
  PaletteItem,
  PaletteProviderDef,
  PerspectiveDef,
  SharedStateDef,
  ViewDef,
} from './define';
import { WORKBENCH_KINDS } from './kinds';

/**
 * Everything an IDE-style app (or a feature of it) contributes: the shell's kinds plus the workbench's. `T` is the app's
 * part of the context (`ShellContext<T>`).
 */
export interface WorkbenchBatch<T extends object = object> extends ContributionBatch<T> {
  readonly views?: readonly ViewDef<T>[];
  readonly activityItems?: readonly ActivityItemDef<T>[];
  // The node and item types are the provider's own; the registry treats them as their base shapes.
  readonly explorerProviders?: readonly ExplorerProviderDef<T, ExplorerNode & any>[]; // eslint-disable-line @typescript-eslint/no-explicit-any
  readonly paletteProviders?: readonly PaletteProviderDef<T, PaletteItem & any>[]; // eslint-disable-line @typescript-eslint/no-explicit-any
  readonly perspectives?: readonly PerspectiveDef<T>[];
  readonly sharedState?: readonly SharedStateDef<T, any>[]; // eslint-disable-line @typescript-eslint/no-explicit-any
  readonly assistantContext?: readonly AssistantContextDef<T>[];
}

const WORKBENCH_KEYS = new Set<string>(WORKBENCH_KINDS.map((k) => k.name));

/** The workbench's kinds in the window's registry. Every `provideWorkbenchContributions` includes it; repeats are harmless. */
export function provideWorkbenchKinds(): EnvironmentProviders {
  return provideContributionKinds(...WORKBENCH_KINDS);
}

/**
 * Registers a batch of shell and workbench contributions through the shell's one registry (ADR 0013 §2: never a second
 * mechanism): `provideWorkbenchContributions<PacenotesContext>(CODE_CONTRIBUTIONS)` with the batch in a
 * `*.contributions.ts` file that imports only `define*` and types.
 */
export function provideWorkbenchContributions<T extends object = object>(batch: WorkbenchBatch<T>): EnvironmentProviders {
  const shell: Record<string, unknown> = {};
  const workbench: [string, readonly unknown[]][] = [];
  for (const [kind, items] of Object.entries(batch)) {
    if (WORKBENCH_KEYS.has(kind)) workbench.push([kind, (items as readonly unknown[] | undefined) ?? []]);
    else shell[kind] = items;
  }
  return makeEnvironmentProviders([
    provideWorkbenchKinds(),
    provideContributions(shell as ContributionBatch<T>),
    ...workbench.flatMap(([kind, items]) => contributionProviders(kind, items)),
  ]);
}
