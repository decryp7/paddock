export default {
  id: 'dyno', name: 'Dyno', icon: 'dyno', status: 'In development', platforms: 'Windows, macOS and Linux',
  accent: { dark: '#F063A8', light: '#B01E6E', fill: '#C2287C' },
  headline: ['See why your PC', 'is slow.'],
  sub: ['Measures your computer, explains the cause in', 'plain words and keeps every change reversible.'],
  shots: { layout: 'suffix', hero: 'dashboard', back: 'processes' },
  features: {
    heading: 'A clear view of your PC, and a calm hand on it.',
    panels: [
      { shot: 'dashboard', crop: [830, 175, 580], title: 'Why is my PC slow?', text: 'The likeliest cause, with the evidence and what you can do about it.' },
      { shot: 'dashboard', crop: [245, 190, 560], title: 'A health score with reasons', text: 'What holds it back, with fixes that preview first.' },
      { shot: 'rules', crop: [245, 585, 750], title: 'Every change has an undo', text: 'Rules ease runaway programs, and every action can be undone.' },
      { shot: 'reports', crop: [250, 165, 660], title: 'A weekly report in plain words', text: 'Highlights and busiest apps, exported as Markdown or a web page.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'pulse', title: 'Sample', text: 'An agent measures while the window is closed.' },
      { glyph: 'search', title: 'Explain', text: 'Names the likeliest cause, with evidence.' },
      { glyph: 'sliders', title: 'Tune with undo', text: 'Only your rules or clicks change anything.' },
      { glyph: 'bell', title: 'Alert', text: 'Alarms for low disk, heat and crash loops.' },
      { glyph: 'doc', title: 'Report', text: 'A weekly summary you can export.' },
    ],
    family: 'Dyno can ask Pitwall to put a diagnosis in plain words, and reports machine health to Pitcrew.',
  },
  worksWith: [
    { app: 'pitwall', note: 'Plain-words diagnosis' },
    { app: 'pitcrew', note: 'Machine health' },
  ],
};
