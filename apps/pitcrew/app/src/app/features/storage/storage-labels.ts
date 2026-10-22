import type { CommandError, EngineKind, PruneKind } from '../../core/api.types';
import { plural } from '../../shared/format';

/** The words for each prune kind: its label, and what it removes. */
export const PRUNE_LABELS: Record<PruneKind, { label: string; hint: string; one: string; many: string }> = {
  stoppedContainers: { label: 'Stopped containers', hint: 'containers that are not running', one: 'container', many: 'containers' },
  danglingImages: { label: 'Dangling images', hint: 'untagged layers nothing refers to', one: 'image', many: 'images' },
  unusedImages: { label: 'Unused images', hint: 'images no container uses', one: 'image', many: 'images' },
  buildCache: { label: 'Build cache', hint: 'cached build steps', one: 'cache entry', many: 'cache entries' },
  volumes: { label: 'Volumes', hint: 'removes data; only unused volumes', one: 'volume', many: 'volumes' },
};

export const ENGINE_NAMES: Record<EngineKind, string> = { docker: 'Docker', podman: 'Podman' };

/** A count of one kind, in words ("3 stopped containers" reads awkwardly, so: "3 containers"). */
export function countOf(kind: PruneKind, n: number): string {
  const l = PRUNE_LABELS[kind];
  return plural(n, l.one, l.many);
}

/** A command error in a sentence for the page, with what to do for `busy`. */
export function errorSentence(e: CommandError): string {
  if (e.code === 'busy') return `A job is running, so nothing is removed right now. ${e.message}`;
  return e.message;
}

