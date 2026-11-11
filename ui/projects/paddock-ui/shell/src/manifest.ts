import type {
  CommandDef,
  DialogDef,
  DialogKind,
  DialogModality,
  HealthServiceDef,
  KeybindingDef,
  MenuItemDef,
  NotificationKindDef,
  PageDef,
  SettingsPageDef,
  StatusItemDef,
} from '@decryp7/paddock-ui/shell/contributions';
import { DIALOG_KINDS } from '@decryp7/paddock-ui/shell/contributions';
import { normalizeKey, toAccelerator } from '@decryp7/paddock-ui/shell/contributions';
import { isMessage, type Message } from '@decryp7/paddock-ui/shell/contributions';

/*
 * `paddock-manifest.json`: the static parts of a window's contributions that Rust needs, generated from the registry by
 * a test in write mode and compared on every run (like Debrief's `dialog-contracts.json`). Rust checks it at build/test
 * time (`paddock_shell::manifest::check`): every menu item routes to a command, every contracted IPC command exists and
 * is open to dialogs, every health service has a supervisor and vice versa, every app settings section has a page and
 * every page the generic form shows has a section. Arrays and records are sorted so the
 * checked-in copy has a stable diff. Change the shape only together with its Rust reader.
 */

export const MANIFEST_VERSION = 1;
/** The conventional file name, next to the app's `tauri.conf.json` (`src-tauri/paddock-manifest.json`). */
export const MANIFEST_FILE = 'paddock-manifest.json';

export interface ManifestCommand {
  readonly id: string;
  /** Message id of the title. */
  readonly title: string;
}

export interface ManifestKeybinding {
  readonly command: string;
  /** Canonical form (`Mod+Shift+P`). */
  readonly key: string;
  /** Native accelerator (`CmdOrCtrl+Shift+P`). */
  readonly accelerator: string;
  readonly scope: string;
}

export interface ManifestMenuItem {
  readonly menu: string;
  /** Also the native menu item id; the native menu event routes it back to this command. */
  readonly command: string;
  readonly group: string;
  readonly order: number;
  /** The command's `window`-scope binding as an accelerator, if any. */
  readonly accelerator: string | null;
}

export interface ManifestDialog {
  readonly kind: DialogKind;
  readonly modality: DialogModality;
  readonly instanced: boolean;
  readonly requiredChoice: boolean;
  /** The width it opens at and its preferred height (`null`: measured), logical px, as the Rust contract declares them. */
  readonly width: number;
  readonly height: number | null;
  /** IPC commands the dialog's window may call beyond the base set (sorted; `prefix*` allowed). */
  readonly commands: readonly string[];
}

export interface ManifestNotificationKind {
  readonly actions: readonly string[];
}

export interface PaddockManifest {
  readonly version: typeof MANIFEST_VERSION;
  readonly commands: readonly ManifestCommand[];
  readonly keybindings: readonly ManifestKeybinding[];
  readonly menus: readonly ManifestMenuItem[];
  readonly pages: readonly string[];
  readonly statusItems: readonly string[];
  readonly settingsPages: readonly string[];
  /** The settings pages without a component of their own: the shell's generic form shows the Rust section of their id. */
  readonly settingsForms: readonly string[];
  readonly dialogs: Readonly<Record<string, ManifestDialog>>;
  readonly healthServices: readonly string[];
  readonly notificationKinds: Readonly<Record<string, ManifestNotificationKind>>;
  /** Every message id of the contributions with its source text (the shell's i18n extracts from here, plan S15). */
  readonly messages: Readonly<Record<string, string>>;
}

/** What the manifest is built from: the registry, or any source with the same lists. */
export interface ManifestSource {
  commands(): readonly CommandDef[];
  keybindings(): readonly KeybindingDef[];
  menus(): readonly MenuItemDef[];
  pages(): readonly PageDef[];
  statusItems(): readonly StatusItemDef[];
  settingsPages(): readonly SettingsPageDef[];
  dialogs(): readonly DialogDef[];
  healthServices(): readonly HealthServiceDef[];
  notificationKinds(): readonly NotificationKindDef[];
}

const byString = (a: string, b: string): number => (a < b ? -1 : a > b ? 1 : 0);
const sortedIds = (items: readonly { id: string }[]): string[] => items.map((i) => i.id).sort(byString);
function sortedRecord<V>(entries: readonly (readonly [string, V])[]): Record<string, V> {
  const out: Record<string, V> = {};
  for (const [k, v] of [...entries].sort((a, b) => byString(a[0], b[0]))) out[k] = v;
  return out;
}

/** The manifest of a set of contributions. Malformed items (which the lint reports) are left out rather than thrown on. */
export function buildManifest(source: ManifestSource): PaddockManifest {
  const keybindings: ManifestKeybinding[] = source
    .keybindings()
    .flatMap((k) => {
      const key = normalizeKey(k.key);
      const accelerator = toAccelerator(k.key);
      return key && accelerator ? [{ command: k.command, key, accelerator, scope: k.scope }] : [];
    })
    .sort((a, b) => byString(a.scope, b.scope) || byString(a.key, b.key) || byString(a.command, b.command));
  const accelerators = new Map<string, string>();
  for (const k of keybindings) if (k.scope === 'window' && !accelerators.has(k.command)) accelerators.set(k.command, k.accelerator);

  const messages: [string, string][] = [];
  const collect = (m: Message | undefined): void => {
    if (isMessage(m)) messages.push([m.id, m.text]);
  };
  for (const c of source.commands()) {
    collect(c.title);
    collect(c.category);
  }
  for (const p of [...source.pages(), ...source.statusItems(), ...source.settingsPages(), ...source.dialogs(), ...source.notificationKinds()]) collect(p.title);
  for (const p of source.settingsPages()) collect(p.subgroup);
  for (const h of source.healthServices()) collect(h.label);

  return {
    version: MANIFEST_VERSION,
    commands: source
      .commands()
      .map((c) => ({ id: c.id, title: isMessage(c.title) ? c.title.id : '' }))
      .sort((a, b) => byString(a.id, b.id)),
    keybindings,
    menus: source
      .menus()
      .map((m) => ({ menu: m.menu, command: m.command, group: m.group, order: m.order, accelerator: accelerators.get(m.command) ?? null }))
      .sort((a, b) => byString(a.menu, b.menu) || a.order - b.order || byString(a.command, b.command)),
    pages: sortedIds(source.pages()),
    statusItems: sortedIds(source.statusItems()),
    settingsPages: sortedIds(source.settingsPages()),
    settingsForms: sortedIds(source.settingsPages().filter((p) => p.load === undefined)),
    dialogs: sortedRecord(
      source
        .dialogs()
        .filter((d) => (DIALOG_KINDS as readonly string[]).includes(d.kind) && d.modality !== undefined)
        .map((d) => [
          d.id,
          {
            kind: d.kind,
            modality: d.modality,
            instanced: d.instanced === true,
            requiredChoice: d.requiredChoice === true,
            width: d.width,
            height: d.height,
            commands: [...new Set(d.commands)].sort(byString),
          } satisfies ManifestDialog,
        ]),
    ),
    healthServices: sortedIds(source.healthServices()),
    notificationKinds: sortedRecord(source.notificationKinds().map((n) => [n.id, { actions: [...n.actions] }])),
    messages: sortedRecord(messages),
  };
}

/** The checked-in text of a manifest: two-space JSON with a final newline. */
export function serializeManifest(manifest: PaddockManifest): string {
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

export type ManifestParse = { readonly ok: true; readonly manifest: PaddockManifest } | { readonly ok: false; readonly errors: readonly string[] };

/** Parses a manifest file (untrusted text) into the typed value, or every reason it is not one. */
export function parseManifest(text: string): ManifestParse {
  let raw: unknown;
  try {
    raw = JSON.parse(text);
  } catch (e) {
    return { ok: false, errors: [`not JSON: ${e instanceof Error ? e.message : String(e)}`] };
  }
  const errors: string[] = [];
  const at = (path: string, why: string) => errors.push(`${path}: ${why}`);
  if (!isRecord(raw)) return { ok: false, errors: ['the manifest is not an object'] };
  if (raw['version'] !== MANIFEST_VERSION) at('version', `expected ${MANIFEST_VERSION}, found ${JSON.stringify(raw['version'])}`);
  const known = ['version', 'commands', 'keybindings', 'menus', 'pages', 'statusItems', 'settingsPages', 'settingsForms', 'dialogs', 'healthServices', 'notificationKinds', 'messages'];
  for (const k of Object.keys(raw)) if (!known.includes(k)) at(k, 'unknown field');

  const list = (key: string, item: (v: unknown, path: string) => void): void => {
    const v = raw[key];
    if (!Array.isArray(v)) return void at(key, 'expected an array');
    v.forEach((x, i) => item(x, `${key}[${i}]`));
  };
  const record = (key: string, item: (v: unknown, path: string) => void): void => {
    const v = raw[key];
    if (!isRecord(v)) return void at(key, 'expected an object');
    for (const [k, x] of Object.entries(v)) item(x, `${key}.${k}`);
  };
  const fields = (v: unknown, path: string, spec: Record<string, (x: unknown) => boolean>): void => {
    if (!isRecord(v)) return void at(path, 'expected an object');
    for (const [k, ok] of Object.entries(spec)) if (!ok(v[k])) at(`${path}.${k}`, 'missing or of the wrong type');
    for (const k of Object.keys(v)) if (!(k in spec)) at(`${path}.${k}`, 'unknown field');
  };
  const str = (x: unknown): boolean => typeof x === 'string';
  const num = (x: unknown): boolean => typeof x === 'number' && Number.isFinite(x);
  const bool = (x: unknown): boolean => typeof x === 'boolean';
  const strs = (x: unknown): boolean => Array.isArray(x) && x.every(str);

  list('commands', (v, p) => fields(v, p, { id: str, title: str }));
  list('keybindings', (v, p) => fields(v, p, { command: str, key: str, accelerator: str, scope: str }));
  list('menus', (v, p) => fields(v, p, { menu: str, command: str, group: str, order: num, accelerator: (x) => x === null || str(x) }));
  for (const key of ['pages', 'statusItems', 'settingsPages', 'settingsForms', 'healthServices']) list(key, (v, p) => void (!str(v) && at(p, 'expected a string')));
  record('dialogs', (v, p) =>
    fields(v, p, {
      kind: (x) => (DIALOG_KINDS as readonly unknown[]).includes(x),
      modality: (x) => x === 'modal' || x === 'modeless',
      instanced: bool,
      requiredChoice: bool,
      width: num,
      height: (x) => x === null || num(x),
      commands: strs,
    }),
  );
  record('notificationKinds', (v, p) => fields(v, p, { actions: strs }));
  record('messages', (v, p) => void (!str(v) && at(p, 'expected a string')));
  return errors.length ? { ok: false, errors } : { ok: true, manifest: raw as unknown as PaddockManifest };
}

/**
 * How the checked-in manifest differs from the generated one, one line per section that changed (empty: up to date).
 * The app's manifest spec writes the file when asked (`PADDOCK_WRITE_MANIFEST=1`) and otherwise fails on any line here.
 */
export function manifestDrift(generated: PaddockManifest, checkedIn: string): string[] {
  const parsed = parseManifest(checkedIn);
  if (!parsed.ok) return parsed.errors.map((e) => `checked-in manifest is invalid: ${e}`);
  const out: string[] = [];
  const have = parsed.manifest as unknown as Record<string, unknown>;
  const want = generated as unknown as Record<string, unknown>;
  for (const key of Object.keys(want)) {
    const a = JSON.stringify(have[key]);
    const b = JSON.stringify(want[key]);
    if (a === b) continue;
    const ids = (v: unknown): string[] =>
      Array.isArray(v) ? v.map((x) => (typeof x === 'string' ? x : JSON.stringify(x))) : isRecord(v) ? Object.keys(v) : [JSON.stringify(v)];
    const before = new Set(ids(have[key]));
    const after = new Set(ids(want[key]));
    const added = [...after].filter((x) => !before.has(x));
    const removed = [...before].filter((x) => !after.has(x));
    const parts = [added.length && `added ${added.join(', ')}`, removed.length && `removed ${removed.join(', ')}`].filter(Boolean);
    out.push(`${key}: ${parts.length ? parts.join('; ') : 'entries changed'}`);
  }
  if (!out.length && serializeManifest(generated) !== checkedIn) out.push('formatting differs (regenerate the file)');
  return out;
}

function isRecord(v: unknown): v is Record<string, unknown> {
  return typeof v === 'object' && v !== null && !Array.isArray(v);
}
