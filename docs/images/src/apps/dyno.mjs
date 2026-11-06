export default {
  id: 'dyno', name: 'Dyno', icon: 'dyno', status: 'In development', platforms: 'Windows, macOS and Linux',
  accent: { dark: '#F063A8', light: '#B01E6E', fill: '#C2287C' },
  headline: ['See why your PC', 'is slow.'],
  sub: ['Measures your computer and explains the cause', 'in plain words, with the evidence.'],
  // The screenshots still show the features ADR 0021 removed; until they are retaken after the move onto the shared shell,
  // the hero, social card and feature panels are drawn illustrations (mocks.mjs), not screenshots.
  illustrated: true,
  social: { fx: 670 }, // the card's right edge falls in the gap between the Memory and Disk tiles and between the cause and alarms cards
  shots: { layout: 'suffix', hero: 'dashboard', back: 'processes' },
  features: {
    heading: 'A clear view of your PC.',
    panels: [
      { mock: 'explain', title: 'Why is my PC slow?', text: 'The likeliest cause in plain words, with the evidence behind it.' },
      { mock: 'normal', title: 'Is this normal?', text: 'Dyno learns what is usual for this PC and says when today is not.' },
      { mock: 'alarms', title: 'Alarms that notify', text: 'Low disk, heat and crash loops, until you acknowledge them.' },
      { mock: 'bench', title: 'Benchmarks on a click', text: 'CPU, memory and disk, measured before and after a change you make.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'pulse', title: 'Sample', text: 'An agent measures while the window is closed.' },
      { glyph: 'search', title: 'Explain', text: 'Names the likeliest cause, with evidence.' },
      { glyph: 'bell', title: 'Alert', text: 'Alarms for low disk, heat and crash loops.' },
      { glyph: 'bars', title: 'Measure', text: 'Benchmarks, counters and a flight recorder.' },
      { glyph: 'doc', title: 'Report', text: 'A weekly summary you can export.' },
    ],
    family: 'Pitwall can read Dyno over MCP once AI access is on; Explain this is planned.',
  },
  worksWith: [
    { app: 'pitwall', note: 'Plain-words diagnosis, later' },
    { app: 'pitcrew', note: 'Machine health, later' },
  ],
};
