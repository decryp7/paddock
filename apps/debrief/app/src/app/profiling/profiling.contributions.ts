import { lazyCommands, type CommandStub } from '../extensions/lazy-registration';
import { PROFILING_VIEW_IDS as V } from './profiling.ids';

// The registration file of the Profiling commands: stubs only; the command classes and the stores they read load when
// the window is idle or a command runs. `extensions/lazy-commands.spec.ts` keeps the stubs equal to the code.

const CATEGORY = $localize`:@@profiling.category:Profiling`;

export const PROFILING_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'profiling.open', title: $localize`:@@profiling.cmd.open:Open performance capture…`, category: 'File' },
  { id: 'profiling.captures', title: $localize`:@@profiling.cmd.captures:Profiling: show captures`, category: CATEGORY, view: V.captures },
  { id: 'profiling.hotspots', title: $localize`:@@profiling.cmd.hotspots:Profiling: hotspots`, category: CATEGORY, view: V.hotspots },
  { id: 'profiling.tree', title: $localize`:@@profiling.cmd.tree:Profiling: call tree`, category: CATEGORY, view: V.tree },
  { id: 'profiling.inverted', title: $localize`:@@profiling.cmd.inverted:Profiling: inverted tree (bottom-up)`, category: CATEGORY, view: V.inverted },
  { id: 'profiling.butterfly', title: $localize`:@@profiling.cmd.butterfly:Profiling: callers and callees`, category: CATEGORY, view: V.butterfly },
  { id: 'profiling.flame', title: $localize`:@@profiling.cmd.flame:Profiling: flame graph`, category: CATEGORY, view: V.flame },
  { id: 'profiling.compare', title: $localize`:@@profiling.cmd.compare:Profiling: compare with a baseline (differential flame graph)`, category: CATEGORY, view: V.compare },
  { id: 'profiling.threads', title: $localize`:@@profiling.cmd.threads:Profiling: thread timeline`, category: CATEGORY, view: V.threads },
  { id: 'profiling.findings', title: $localize`:@@profiling.cmd.findings:Profiling: findings`, category: CATEGORY, view: V.findings },
  { id: 'profiling.heap', title: $localize`:@@profiling.cmd.heap:Profiling: memory (heap snapshots)`, category: CATEGORY, view: V.heap },
];

export const DOTNET_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'profiling.dotnet', title: $localize`:@@profiling.cmd.dotnet:Profile .NET process…`, category: 'Tools' },
];

export const NATIVE_COMMAND_STUBS: readonly CommandStub[] = [
  { id: 'profiling.native', title: $localize`:@@profiling.cmd.native:Profile native process…`, category: 'Tools' },
  { id: 'profiling.etl', title: $localize`:@@profiling.cmd.etl:Open process from .etl…`, category: 'File' },
];

export const PROFILING_COMMANDS_CONTRIBUTIONS = [
  ...lazyCommands(() => import('./profiling-commands').then((m) => m.ProfilingCommands), PROFILING_COMMAND_STUBS),
  ...lazyCommands(() => import('./dotnet/dotnet-commands').then((m) => m.DotnetCommands), DOTNET_COMMAND_STUBS),
  ...lazyCommands(() => import('./native/native-commands').then((m) => m.NativeCommands), NATIVE_COMMAND_STUBS),
];
