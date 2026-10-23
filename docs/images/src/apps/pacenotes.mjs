export default {
  id: 'pacenotes', name: 'Pacenotes', icon: 'pacenotes', planned: true, status: 'Planned', platforms: 'Windows, macOS and Linux',
  accent: { dark: '#EE6677', light: '#831826', fill: '#831826' },
  headline: ['Understand your', 'code, locally.'],
  sub: ['Navigate, review, diagram and trace a repository,', 'and serve it to local AI over MCP.'],
  features: {
    heading: 'Read a repository the way a maintainer would, without running it.',
    panels: [
      { mock: 'graph', title: 'Understand', text: 'Symbols, references, call graphs and a token-budgeted repo map.' },
      { mock: 'review', title: 'Review', text: 'Rule packs and an optional local-AI pass over a diff, with SARIF output.' },
      { mock: 'diagram', title: 'Diagrams that keep your edits', text: 'C4, module and sequence diagrams as draw.io files.' },
      { mock: 'mcp', title: 'Serve local AI over MCP', text: 'Read-only, with secrets redacted from what it returns.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'layers', title: 'Index', text: 'Reads the repository without running it.' },
      { glyph: 'graph', title: 'Understand', text: 'Symbols, references and call graphs.' },
      { glyph: 'branch', title: 'Review and trace', text: 'Diffs, diagrams and requirements.' },
      { glyph: 'server', title: 'Serve over MCP', text: 'Local AI gets the same view.' },
    ],
    family: 'Pitwall calls it over MCP, Debrief checks log lines against it and Pitcrew runs it in CI.',
  },
  worksWith: [
    { app: 'pitwall', note: 'Calls it over MCP' },
    { app: 'debrief', note: 'Checks log lines' },
    { app: 'sighter', note: 'Drafts test plans' },
    { app: 'pitcrew', note: 'Runs it in CI' },
  ],
};
