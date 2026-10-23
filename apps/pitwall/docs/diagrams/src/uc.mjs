import { useCases } from './uc-lib.mjs';
useCases('uml-use-cases-library', {
  title: 'Use cases — models and engines', desc: 'What the machine owner does to get models running: llama.cpp, models, names for them and what they cost in memory.', bound: 'Pitwall', owner: 'Machine owner',
  rows: ['Install or update llama.cpp', 'Download a model', 'Review model update notices', 'Set a preset or alias', 'Set parallel slots and see their cost', 'Pin or roll back an engine build', 'Delete or keep-loaded a model', 'Add a model catalog pack'],
  hubRow: 1, sats: [{ text: 'Self-test the build', from: 0, row: 0, kind: '«include»' }, { text: 'Check that it fits', from: 1, row: 1, kind: '«include»' }, { text: 'Show features per build', from: 5, row: 5, kind: '«include»' }],
});
useCases('uml-use-cases-operate', {
  title: 'Use cases — running Pitwall', desc: 'Day to day: who may use it, how much memory it may take, what it has done and how to drive it from a terminal.', bound: 'Pitwall', owner: 'Machine owner',
  rows: ['Approve or deny a pairing', 'Manage app keys, permissions and limits', 'See usage per app over time', 'Choose a memory preset', 'Pause serving or release memory', 'Turn offline mode on or off', 'Watch the dashboard and requests', 'Try models in the playground', 'Run a benchmark', 'Use the pitwall command line'],
  hubRow: 1, sats: [{ text: 'Set a token or request limit', from: 1, row: 1, kind: '«extend»' }, { text: 'Compare tokens per second', from: 8, row: 8, kind: '«include»' }],
});
useCases('uml-use-cases-clients', {
  title: 'Use cases — apps and tools', desc: 'What a suite app or AI tool asks of Pitwall, and what the OS does to keep it running. Included steps are done by Pitwall itself.', bound: 'Pitwall', owner: 'Suite app or AI tool',
  rows: ['Pair with Pitwall', 'Chat, streaming or not', 'Create embeddings', 'Pick a model by alias or capability', 'Cancel a job', 'Count tokens, run a task', 'Start and restart the server'],
  hubRow: 1, sats: [{ text: 'Authorise and queue', from: 0, row: 0, kind: '«include»' }, { text: 'Load the model on first use', from: 1, row: 1, kind: '«include»' }, { text: 'Record usage', from: 2, row: 2, kind: '«include»' }], ext: { name: 'OS service manager', row: 6 },
});
