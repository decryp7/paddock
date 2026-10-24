import { Injector } from '@angular/core';
import { ContributionRegistry, safeIdentity, type ContributionLookup } from '@decryp7/paddock-ui/shell';

/**
 * Every defect of the window's contributions, one sentence each (empty: valid). The one lint over every instance
 * (ADR 0012 §2, rule 40): duplicate ids, key conflicts within a scope, menu items and keys naming a missing command,
 * missing icons and message ids, dialogs without a kind or with a modality their kind forbids, `min > pref` sizes,
 * unknown batch keys, and every kind's own checks (the workbench's kinds included).
 */
export function contributionProblems(injector: Injector): string[] {
  const registry = injector.get(ContributionRegistry);
  const out: string[] = [];
  for (const key of registry.unknownKinds()) out.push(`provideContributions: unknown contribution kind ${JSON.stringify(key)}`);
  for (const key of registry.shadowedKinds()) out.push(`provideContributionKinds: a kind named ${JSON.stringify(key)} is already registered`);
  const lookup: ContributionLookup = registry;
  for (const kind of registry.kinds) {
    const seen = new Set<string>();
    const reported = new Set<string>();
    for (const [i, item] of registry.raw(kind.name).entries()) {
      const id = safeIdentity(kind, item);
      if (id === null) {
        out.push(`${kind.label} #${i + 1} is not a ${kind.label} (define it with its define* function)`);
        continue;
      }
      if (seen.has(id)) {
        if (!reported.has(id)) out.push(`duplicate ${kind.label} ${id}`);
        reported.add(id);
        continue;
      }
      seen.add(id);
      for (const problem of safeCheck(() => kind.check?.(item as never, lookup) ?? [])) out.push(`${kind.label} ${id}: ${problem}`);
    }
    out.push(...safeCheck(() => kind.checkAll?.(registry.items(kind.name) as never, lookup) ?? []));
  }
  return out;
}

/** Throws one error listing every defect of the window's contributions; every app's spec runs it on each window's providers. */
export function assertContributionsValid(injector: Injector): void {
  const problems = contributionProblems(injector);
  if (problems.length) throw new Error(`Invalid contributions (${problems.length}):\n- ${problems.join('\n- ')}`);
}

function safeCheck(check: () => readonly string[]): readonly string[] {
  try {
    return check();
  } catch (e) {
    return [`a lint check threw: ${e instanceof Error ? e.message : String(e)}`];
  }
}
