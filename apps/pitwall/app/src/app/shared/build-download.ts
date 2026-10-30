import type { CatalogBuild } from '../core/api.types';

/**
 * The name the server gives a catalog build's download (`engine_setup::start_install`: "llama.cpp <label> (<tag>)", the
 * label being the catalog's): how the page tells which catalog build a running download installs.
 */
export function buildDownloadName(engineName: string, build: Pick<CatalogBuild, 'label' | 'tag'>): string {
  return `${engineName} ${build.label} (${build.tag})`;
}
