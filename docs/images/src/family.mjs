// The one place that names the codebase-understanding tool. Working name, still open: to rename it run
//   node docs/images/src/rename-codebase-tool.mjs <NewName>
// which updates this constant, README.md and docs/ecosystem.md, and regenerates both images.
export const CODEBASE_TOOL = 'Pacenotes';

// Status chip per app on the hero (and the wording used in the README and ecosystem tables). tone: ok | soon | dev | design.
export const STATUS = {
  pitcrew: { text: 'v1.0', tone: 'ok' },
  debrief: { text: 'Releasing', tone: 'soon' },
  pitwall: { text: 'Release next', tone: 'soon' },
  dyno: { text: 'In development', tone: 'dev' },
  spotter: { text: 'In development', tone: 'dev' },
  sighter: { text: 'In design', tone: 'design' },
  pacenotes: { text: 'In design', tone: 'design' },
};
