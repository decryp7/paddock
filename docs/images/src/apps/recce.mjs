export default {
  // No folder under apps/ until the app has code (a folder there needs its paddock.ci.json, scripts/affected.test.mjs).
  id: 'recce', name: 'Recce', icon: 'recce', home: 'docs/planned/recce', planned: true, status: 'Planned', platforms: 'Windows, macOS and Linux',
  accent: { dark: '#57D3A6', light: '#0B7A55', fill: '#0B7A55' },
  headline: ['Know where', 'everything is.'],
  sub: ['One search box over your files, documents, email', 'and SharePoint, by name or by meaning, with', 'answers that cite their sources.'],
  features: {
    heading: 'Search the whole PC from one box, and see where every answer came from.',
    panels: [
      { mock: 'search', title: 'One box for everything', text: 'Files, document content, email and attachments, SharePoint and OneDrive.' },
      { mock: 'meaning', title: 'Search by meaning', text: 'Finds the passage you mean even when it uses other words.' },
      { mock: 'answer', title: 'Answers with sources', text: 'A local model answers from your own documents and cites each one.' },
      { mock: 'launcher', title: 'A light launcher', text: 'Apps, files, a calculator and actions from one hotkey.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'layers', title: 'Index', text: 'Names, content and meaning, on your PC.' },
      { glyph: 'search', title: 'Search', text: 'One box, as you type.' },
      { glyph: 'chat', title: 'Answer', text: 'A local model, with cited sources.' },
      { glyph: 'server', title: 'Serve over MCP', text: 'Later: local AI gets the same view.' },
    ],
    family: 'Pitwall runs the local model for answers; Pacenotes shares the document readers.',
  },
  worksWith: [
    { app: 'pitwall', note: 'Runs the local model' },
    { app: 'pacenotes', note: 'Shares document readers' },
  ],
};
