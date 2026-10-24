import type { Message, ShellContext } from '@decryp7/paddock-ui/shell';
import { definePaletteProvider, type PaletteItem, type PaletteProviderDef } from '@decryp7/paddock-ui/workbench';

/*
 * A palette provider over an engine's `names_search` command (paddock-names, plan X18; rule 20): the user types the name
 * however it is written and the engine finds it. The wire shapes mirror `paddock_names::service` (camelCase JSON).
 */

/** How a name matched, best first (`paddock_names::How`). */
export type NameHow = 'userAlias' | 'exact' | 'joined' | 'parts' | 'alias' | 'prefix' | 'words' | 'typo';

/** The arguments of `names_search`. */
export interface NamesSearchRequest {
  readonly query: string;
  /** 1 to 50; absent means 10. */
  readonly limit?: number;
}

/** One match of `names_search`. */
export interface NameMatch {
  readonly id: string;
  readonly display: string;
  readonly how: NameHow;
  readonly score: number;
  /** A phrase saying why it matched. */
  readonly why: string;
}

/** The reply of `names_search`. */
export interface NamesSearchReply {
  readonly matches: readonly NameMatch[];
  /** A question for a true tie ("By 'x', do you mean A or B?") with the options' ids. */
  readonly clarify: { readonly question: string; readonly options: readonly string[] } | null;
  /** How many names are searchable (0 while the first catalogue is being built). */
  readonly searchable: number;
}

/** The engine's command name (`paddock_names::NAMES_SEARCH`). */
export const NAMES_SEARCH = 'names_search';
/** The most matches `names_search` returns (`paddock_names::MAX_MATCHES`). */
export const NAMES_MAX_MATCHES = 50;
/** The longest query `names_search` takes (`paddock_names::MAX_TEXT_CHARS`). */
export const NAMES_MAX_QUERY = 256;

/** A palette result of a names search: the match is kept for `accept`. */
export interface NamePaletteItem extends PaletteItem {
  readonly match: NameMatch | null;
}

export interface NamesPaletteOptions<T extends object> {
  readonly id: string;
  readonly title: Message;
  readonly prefix?: string;
  readonly placeholder?: Message;
  readonly order: number;
  readonly icon?: string;
  readonly when?: (ctx: ShellContext<T>) => boolean;
  /** Calls the engine's `names_search` (through the app's IPC backend). */
  readonly search: (request: NamesSearchRequest, ctx: ShellContext<T>, signal: AbortSignal) => Promise<NamesSearchReply>;
  /** Opens what the user picked. */
  readonly accept: (match: NameMatch, ctx: ShellContext<T>) => void | Promise<void>;
  /** Text while the first catalogue is being built; defaults to "Still indexing names…". */
  readonly building?: Message;
  /** Debounce in ms (engine queries; default 80). */
  readonly debounceMs?: number;
}

/** The palette rows of a reply: a tie's question first (inert), then the matches with why they matched. */
export function namesToItems(reply: NamesSearchReply, icon?: string, building?: string): NamePaletteItem[] {
  const items: NamePaletteItem[] = [];
  if (reply.searchable === 0 && building) items.push({ id: 'pd.names.building', label: building, disabled: building, match: null });
  if (reply.clarify?.question) items.push({ id: 'pd.names.clarify', label: reply.clarify.question, disabled: reply.clarify.question, match: null });
  for (const m of reply.matches.slice(0, NAMES_MAX_MATCHES)) items.push({ id: m.id, label: m.display, detail: m.why, ...(icon ? { icon } : {}), match: m });
  return items;
}

/** A palette provider over `names_search` (the matching is the engine's, never TypeScript's). */
export function namesPaletteProvider<T extends object = object>(options: NamesPaletteOptions<T>): PaletteProviderDef<T, NamePaletteItem> {
  return definePaletteProvider<T, NamePaletteItem>({
    id: options.id,
    title: options.title,
    prefix: options.prefix ?? '',
    ...(options.placeholder ? { placeholder: options.placeholder } : {}),
    order: options.order,
    ...(options.when ? { when: options.when } : {}),
    debounceMs: options.debounceMs ?? 80,
    search: async (query, ctx, request) => {
      const text = query.trim().slice(0, NAMES_MAX_QUERY);
      if (!text) return [];
      const reply = await options.search({ query: text, limit: Math.max(1, Math.min(NAMES_MAX_MATCHES, request.limit)) }, ctx, request.signal);
      const building = options.building?.text ?? $localize`:@@pd.wb.palette.names-building:Still indexing names…`;
      return namesToItems(reply, options.icon, building);
    },
    accept: (item, ctx) => (item.match ? options.accept(item.match, ctx) : undefined),
  });
}
