// Debrief marketing art. Screenshots come from the Debrief checkout (docs/screenshots: <name>.png dark, <name>-light.png light).
// Run: PADDOCK_APPS=<dir holding a "debrief" checkout> node docs/images/src/app-poster.mjs debrief --png
// Copy and facts come from the Debrief README; crops are [x, y, width] in 1440x900 CSS pixels (height = width / 2.4).
export default {
  id: 'debrief', name: 'Debrief', icon: 'debrief', status: 'Version 1', platforms: 'Windows, macOS and Linux',
  accent: { dark: '#5B9BFF', light: '#1D5FD1', fill: '#2F6FE0' },
  headline: ['Make sense of', 'any log file.'],
  sub: ['An IDE for analysing logs and diagnostic files,', 'on your machine.'],
  shots: { layout: 'light-suffix', hero: 'workspace', back: 'metrics' },
  features: {
    heading: 'From a pile of files to the one line that matters.',
    panels: [
      { shot: 'workspace', crop: [324, 150, 1087], title: 'Every process on one timeline', text: 'A lane per process and machine, with crashes, errors and starts marked.' },
      { shot: 'patterns', crop: [320, 122, 1091], title: 'Patterns with trends', text: 'Thousands of lines become a short list, each with its own sparkline.' },
      { shot: 'outliers', crop: [320, 122, 1091], title: 'What the errors have in common', text: 'Values over-represented among errors, by lift.' },
      { shot: 'compare', crop: [320, 122, 1091], title: 'Compare two windows', text: 'See which fields changed before and after the incident.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'folder', title: 'Open', text: 'Logs, zips and diagnostic files, or a time range of a huge folder.' },
      { glyph: 'search', title: 'Search and filter', text: 'Facets, lenses and a query language.' },
      { glyph: 'pulse', title: 'Analyse', text: 'Anomalies, patterns, exceptions and error outliers.' },
      { glyph: 'doc', title: 'Share', text: 'Incident reports and redacted copies.' },
    ],
    family: 'Pitcrew opens its job logs in Debrief. Sighter run bundles and Pitwall explanations come after version 1.',
  },
  worksWith: [
    { app: 'pitcrew', note: 'Job logs' },
    { app: 'sighter', note: 'Run bundles (planned)' },
    { app: 'pitwall', note: 'Explanations (after v1)' },
  ],
};
