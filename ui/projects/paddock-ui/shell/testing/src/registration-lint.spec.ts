import { assertRegistrationsLazy, checkRegistrationSource, isRegistrationFile, staticImports } from './registration-lint';

const FILE = 'src/app/models/models.contributions.ts';
const check = (source: string, allow?: readonly string[]) => checkRegistrationSource(FILE, source, allow);

/** A registration file as it should be: define* and ids, types only, everything else behind import(). */
const LAZY = `
import { defineCommand, definePage, msg } from '@decryp7/paddock-ui/shell';
import type { PitwallContext } from '../app-context';
import { type ModelsStore, MODEL_IDS } from './models.ids';
// import { ModelsStore } from './models.store';  (comments are ignored)
/* import { ModelsService } from './models.service'; */
export const MODELS = {
  commands: [defineCommand<PitwallContext>({ id: 'pw.models.refresh', title: msg('pw.models.refresh', 'Refresh'), run: (ctx) => ctx.models.refresh() })],
  pages: [definePage({ id: 'pw.models', title: msg('pw.models', 'Models'), icon: 'pw:model', order: 200, load: () => import('./models-page').then((m) => m.ModelsPage) })],
};
export const text = "import { X } from './x.store'";
export type { ModelsStore };
`;

describe('checkRegistrationSource (lazy by construction)', () => {
  it('passes a registration file of data, types and loaders', () => {
    expect(check(LAZY)).toEqual([]);
  });

  it('fails on importing a store class', () => {
    expect(check(`import { ModelsStore } from './models.state';`)).toEqual([
      `${FILE}:1: imports ModelsStore from ./models.state; stores, services and commands are reached through the context, never imported here`,
    ]);
  });

  it('fails on importing anything from a store, service or backend module', () => {
    expect(check(`\nimport { initialModels } from './models.store';\nimport { x } from '../core/models-backend';`)).toEqual([
      `${FILE}:2: imports initialModels from ./models.store, which is eager; reach it through the context or import() in a loader`,
      `${FILE}:3: imports x from ../core/models-backend, which is eager; reach it through the context or import() in a loader`,
    ]);
  });

  it('fails on importing a service class', () => {
    expect(check(`import { DownloadService } from '../core/downloads';`)).toHaveLength(1);
  });

  it('fails on importing a command class or command module', () => {
    expect(check(`import {\n  ModelCommands,\n} from './model-actions';`)).toEqual([
      `${FILE}:1: imports ModelCommands from ./model-actions; stores, services and commands are reached through the context, never imported here`,
    ]);
    expect(check(`import { refresh } from './models.commands';`)).toHaveLength(1);
  });

  it('fails on importing a component instead of loading it', () => {
    expect(check(`import { ModelsPage } from './models-page';`)).toEqual([
      `${FILE}:1: imports the component ModelsPage from ./models-page; load it with import() in the contribution's load`,
    ]);
  });

  it('fails on a renamed store import', () => {
    expect(check(`import { ModelsStore as models } from './state';`)).toHaveLength(1);
    expect(check(`import { create as CreateDialog } from './state';`)).toHaveLength(1);
  });

  it('fails on namespace, default and side-effect imports', () => {
    expect(check(`import * as models from './models';\nimport models2 from './models2';\nimport './register-models';`)).toEqual([
      `${FILE}:1: namespace import of ./models pulls in the whole module; import the define* and ids you use`,
      `${FILE}:2: default import of ./models2 pulls in the whole module; import the define* and ids you use`,
      `${FILE}:3: side-effect import of ./register-models runs it at start-up; a registration file holds data and loaders only`,
    ]);
  });

  it('fails on re-exporting a store', () => {
    expect(check(`export { ModelsStore } from './models.state';`)).toHaveLength(1);
    expect(check(`export * from './models.store';`)).toHaveLength(1);
    expect(check(`export type { ModelsStore } from './models.store';`)).toEqual([]);
  });

  it('takes an allow list for modules that only look eager', () => {
    expect(check(`import { MODEL_COMMAND_IDS } from './models.commands';`, ['./models.commands'])).toEqual([]);
  });

  it('applies to *.contributions.ts files only and throws listing every defect', () => {
    expect(isRegistrationFile(FILE)).toBe(true);
    expect(isRegistrationFile('src/app/models/models.store.ts')).toBe(false);
    expect(() => assertRegistrationsLazy({ [FILE]: LAZY, 'src/app/models/models.store.ts': `import { HttpService } from './http';` })).not.toThrow();
    expect(() => assertRegistrationsLazy({ [FILE]: `import { ModelsStore } from './x';` })).toThrow(/^Eager registration files \(1\):\n- src\/app/);
  });

  it('lists static imports with their value bindings', () => {
    expect(staticImports(`import Def, { a, type B, c as d } from 'm';\nimport type { T } from 't';`)).toEqual([
      { specifier: 'm', values: ['default', 'a', 'd'], sideEffect: false, line: 1 },
      { specifier: 't', values: [], sideEffect: false, line: 2 },
    ]);
  });
});
