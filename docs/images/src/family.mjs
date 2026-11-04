// The one place that names the codebase-understanding tool. Working name, still open: to rename it run
//   node docs/images/src/rename-codebase-tool.mjs <NewName>
// which updates this constant, README.md and docs/ecosystem.md, and regenerates both images.
export const CODEBASE_TOOL = 'Pacenotes';

// Status chip per app on the hero and the app posters (README and ecosystem tables use the same wording by hand). To flip a status,
// edit it here and regenerate. tone: ok | soon | dev | design.
export const STATUS = {
  pitcrew: { text: 'v1.0.1', tone: 'ok' },
  pitwall: { text: 'v1.0.3', tone: 'ok' },
  debrief: { text: 'v1.1.1', tone: 'ok' },
  spotter: { text: 'In development', tone: 'dev' },
  dyno: { text: 'In development', tone: 'dev' },
  sighter: { text: 'Planned', tone: 'design' },
  pacenotes: { text: 'Planned', tone: 'design' },
  recce: { text: 'Planned', tone: 'design' },
  scrutineer: { text: 'Planned', tone: 'design' },
};
