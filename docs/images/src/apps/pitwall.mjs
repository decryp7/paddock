export default {
  id: 'pitwall', name: 'Pitwall', icon: 'pitwall', status: 'Nearing release', platforms: 'Windows, macOS and Linux',
  accent: { dark: '#A58BFA', light: '#6D3FD6', fill: '#6D3FD6' },
  headline: ['One private AI', 'for every app.'],
  sub: ['Local models behind the OpenAI, Anthropic and', 'Ollama APIs. Nothing leaves your computer.'],
  shots: { layout: 'suffix', hero: 'dashboard', back: 'models-discover' },
  features: {
    heading: 'Run models on your own machine and share them with every app.',
    panels: [
      { shot: 'dashboard', crop: [250, 415, 740], title: 'Models load when they are needed', text: 'Released when idle or when memory runs short, never while answering.' },
      { shot: 'requests-detail', crop: [255, 140, 780], title: 'See every request', text: 'Which app asked, which model answered and how long it took.' },
      { shot: 'models-discover', crop: [740, 610, 690], title: 'Know if it fits before you download', text: 'A fit check per quantisation, and downloads verified by SHA-256.' },
      { shot: 'models-slots', crop: [710, 322, 520], title: 'Serve more at once', text: 'Choose parallel slots and see the memory each one adds.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'code', title: 'Apps call one API', text: 'OpenAI, Anthropic and Ollama on 127.0.0.1.' },
      { glyph: 'queue', title: 'Queue by priority', text: 'Interactive first, then background and batch.' },
      { glyph: 'cube', title: 'Load on demand', text: 'The model loads only if it fits in memory.' },
      { glyph: 'chat', title: 'Answer and cache', text: 'Streamed replies; prompt prefixes reused.' },
      { glyph: 'chip', title: 'Release memory', text: 'Freed when idle or when pressure rises.' },
    ],
    family: 'Debrief, Spotter, Dyno and Sighter use it for AI; Pacenotes is served to it over MCP.',
  },
  worksWith: [
    { app: 'debrief', note: 'Explain and research' },
    { app: 'spotter', note: 'Release summaries' },
    { app: 'dyno', note: 'Plain-words diagnosis' },
    { app: 'sighter', note: 'Vision models' },
    { app: 'pacenotes', note: 'Code tools over MCP' },
  ],
};
