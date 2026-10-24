/**
 * Lazy by construction (plan §9): a registration file (`*.contributions.ts`) is static data plus loaders, so it may
 * import `define*`, `msg`, ids and types, and reach everything else through `import()` or the context. A static value
 * import of a store, service, backend, facade, registry or command class, or of a component, pulls that code (and what it
 * imports) into the first-paint bundle of every window that registers the file.
 */

/** Module specifiers a registration file must not import values from: stores, services, backends, command modules. */
const EAGER_MODULE = /(?:^|[/.-])(?:store|stores|service|services|backend|backends|facade|commands?)(?:\.ts)?$/i;
/** Imported names that are (by convention) classes with work behind them. */
const EAGER_CLASS = /^[A-Z][A-Za-z0-9]*(?:Store|Service|Backend|Facade|Registry|Commands)$/;
/** Imported names that are (by convention) components; they are loaded with `import()` in `load`. */
const COMPONENT_CLASS = /^[A-Z][A-Za-z0-9]*(?:Component|Page|Dialog|View|Panel|Screen)$/;

/** What a registration file imports, parsed from its source. */
interface StaticImport {
  readonly specifier: string;
  /** Value bindings (type-only ones left out); `*` for a namespace, `default` for a default import. */
  readonly values: readonly string[];
  readonly sideEffect: boolean;
  /** `import type` / `export type`: erased, it never links the module. */
  readonly typeOnly: boolean;
  readonly line: number;
}

/** Whether a path names a registration file. */
export function isRegistrationFile(path: string): boolean {
  return /\.contributions\.ts$/.test(path);
}

/** Every static import and re-export of a TypeScript source (comments are ignored). */
export function staticImports(source: string): StaticImport[] {
  const code = stripComments(source);
  const out: StaticImport[] = [];
  // An import clause never holds `=`, `(` or `;`, which keeps a declaration such as `export const x = ...` from matching.
  const re = /^[ \t]*(import|export)\s+(type\s+)?([^;'"=()]*?)\s*from\s*(['"])([^'"\n]+)\4|^[ \t]*import\s*(['"])([^'"\n]+)\6/gm;
  for (let m = re.exec(code); m; m = re.exec(code)) {
    const line = code.slice(0, m.index).split('\n').length;
    if (m[7] !== undefined) {
      out.push({ specifier: m[7], values: [], sideEffect: true, typeOnly: false, line });
      continue;
    }
    const clause = (m[3] ?? '').trim();
    if (m[1] === 'export' && !/^(\*|\{)/.test(clause)) continue;
    out.push({ specifier: m[5] ?? '', values: m[2] !== undefined ? [] : valueBindings(clause), sideEffect: false, typeOnly: m[2] !== undefined, line });
  }
  return out;
}

/**
 * The defects of one registration file (empty: lazy). `allow` lists module specifiers whose value imports are fine
 * although their name looks eager (a shared constants module named `commands.ids.ts`, say).
 */
export function checkRegistrationSource(path: string, source: string, allow: readonly string[] = []): string[] {
  const out: string[] = [];
  for (const imp of staticImports(source)) {
    if (allow.includes(imp.specifier)) continue;
    const where = `${path}:${imp.line}`;
    if (imp.sideEffect) {
      out.push(`${where}: side-effect import of ${imp.specifier} runs it at start-up; a registration file holds data and loaders only`);
      continue;
    }
    if (!imp.values.length) continue;
    if (EAGER_MODULE.test(imp.specifier)) {
      out.push(`${where}: imports ${imp.values.join(', ')} from ${imp.specifier}, which is eager; reach it through the context or import() in a loader`);
      continue;
    }
    for (const name of imp.values) {
      if (EAGER_CLASS.test(name)) out.push(`${where}: imports ${name} from ${imp.specifier}; stores, services and commands are reached through the context, never imported here`);
      else if (COMPONENT_CLASS.test(name)) out.push(`${where}: imports the component ${name} from ${imp.specifier}; load it with import() in the contribution's load`);
      else if (name === '*' || name === 'default') out.push(`${where}: ${name === '*' ? 'namespace' : 'default'} import of ${imp.specifier} pulls in the whole module; import the define* and ids you use`);
    }
  }
  return out;
}

/** Throws one error listing every eager import of the registration files among `files` (path → source). */
export function assertRegistrationsLazy(files: Readonly<Record<string, string>>, allow: readonly string[] = []): void {
  const problems = Object.entries(files)
    .filter(([path]) => isRegistrationFile(path))
    .flatMap(([path, source]) => checkRegistrationSource(path, source, allow));
  if (problems.length) throw new Error(`Eager registration files (${problems.length}):\n- ${problems.join('\n- ')}`);
}

/** The relative modules a source loads with `import('./x')` (as written). */
export function dynamicImports(source: string): string[] {
  return [...stripComments(source).matchAll(/\bimport\s*\(\s*(['"])(\.{1,2}\/[^'"\n]+)\1\s*\)/g)].map((m) => m[2] as string);
}

/** `spec` (relative, without extension) resolved against the folder of `from`, as a `.ts` path. */
function resolveRelative(from: string, spec: string): string {
  const parts = from.split('/').slice(0, -1);
  for (const seg of spec.split('/')) {
    if (seg === '..') parts.pop();
    else if (seg !== '.') parts.push(seg);
  }
  const path = parts.join('/');
  return path.endsWith('.ts') ? path : `${path}.ts`;
}

/**
 * Lazy chunks stay lazy (plan S16): a module that the entry's own code loads with `import()` (a status item's or a
 * page's component) is a chunk of its own only while the entry's public API does not link it. A value re-export of it,
 * or an `export { type X }` (which keeps the module linked), pulls it into the first paint of every window that
 * imports the entry; `export type { X }` does not. `files` maps the entry's paths (its `public-api.ts` among them,
 * all under one root) to their sources.
 */
export function lazyExportProblems(files: Readonly<Record<string, string>>): string[] {
  const out: string[] = [];
  const lazy = new Set<string>();
  for (const [path, source] of Object.entries(files)) for (const spec of dynamicImports(source)) lazy.add(resolveRelative(path, spec));
  for (const [path, source] of Object.entries(files)) {
    if (!path.endsWith('public-api.ts')) continue;
    for (const imp of staticImports(source)) {
      if (!imp.specifier.startsWith('.') || imp.typeOnly) continue;
      const target = resolveRelative(path, imp.specifier);
      if (!lazy.has(target)) continue;
      const what = imp.values.length ? imp.values.join(', ') : 'types';
      out.push(`${path}:${imp.line}: re-exports ${what} from ${imp.specifier}, which is loaded with import(); export its class with \`export type { … }\` and its helpers from another file, or it loads with every window`);
    }
  }
  return out;
}

function valueBindings(clause: string): string[] {
  const out: string[] = [];
  const brace = clause.indexOf('{');
  const head = (brace >= 0 ? clause.slice(0, brace) : clause).replace(/,\s*$/, '').trim();
  for (const part of head.split(',').map((x) => x.trim()).filter(Boolean)) out.push(part.startsWith('*') ? '*' : 'default');
  if (brace >= 0) {
    for (const part of clause.slice(brace + 1, clause.lastIndexOf('}')).split(',')) {
      const p = part.trim();
      if (!p || /^type\s/.test(p)) continue;
      const [original = '', alias] = p.split(/\s+as\s+/).map((x) => x.trim());
      // A renamed import is judged by both names: `ModelsStore as models` is still a store.
      out.push(alias && !(EAGER_CLASS.test(original) || COMPONENT_CLASS.test(original)) ? alias : original);
    }
  }
  return out;
}

/** Source with comments blanked out (newlines kept, so line numbers stay right); string contents are kept. */
function stripComments(source: string): string {
  let out = '';
  let i = 0;
  while (i < source.length) {
    const c = source[i];
    const n = source[i + 1];
    if (c === '/' && n === '/') {
      while (i < source.length && source[i] !== '\n') i++;
    } else if (c === '/' && n === '*') {
      i += 2;
      while (i < source.length && !(source[i] === '*' && source[i + 1] === '/')) out += source[i++] === '\n' ? '\n' : '';
      i += 2;
    } else if (c === '"' || c === "'" || c === '`') {
      const q = c;
      out += source[i++];
      while (i < source.length && source[i] !== q) {
        if (source[i] === '\\') out += source[i++] ?? '';
        out += source[i++] ?? '';
      }
      out += source[i++] ?? '';
    } else out += source[i++];
  }
  return out;
}
