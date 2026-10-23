export default {
  id: 'sighter', name: 'Sighter', icon: 'sighter', planned: true, status: 'Planned', platforms: 'Windows, macOS and Linux',
  accent: { dark: '#00B8D9', light: '#00788F', fill: '#00788F' },
  headline: ['Find any UI', 'element by sight.'],
  sub: ['Vision-based UI automation that works alongside your', 'FlaUI tests. Everything runs on your machine.'],
  features: {
    heading: 'Test any interface, even the ones no tool can read.',
    panels: [
      { mock: 'detect', title: 'Find elements from pixels', text: 'Boxes, confidence and which layer answered.' },
      { mock: 'review', title: 'Review what it is unsure of', text: 'A queue for everything not identified automatically.' },
      { mock: 'flaui', title: 'Works with your existing UI tests', text: 'Keep UIA where it works; let Sighter see the rest.' },
      { mock: 'apps', title: 'Across apps', text: 'One test, several applications, each with its own model.' },
    ],
  },
  how: {
    steps: [
      { glyph: 'record', title: 'Record', text: 'Windows or screens, plus input events.' },
      { glyph: 'scan', title: 'Identify', text: 'OCR, templates, a detector and a vision model.' },
      { glyph: 'checklist', title: 'Review', text: 'Confirm what was not identified.' },
      { glyph: 'graph', title: 'Train', text: 'A small detector per app, versioned.' },
      { glyph: 'crosshair', title: 'Locate', text: 'Your tests ask; it answers with a box.' },
    ],
    family: 'Pitwall supplies the vision model, Debrief opens run bundles and Pitcrew runs the tests.',
  },
  worksWith: [
    { app: 'pitwall', note: 'Vision models' },
    { app: 'debrief', note: 'Run bundles' },
    { app: 'pitcrew', note: 'Tests on CI runners' },
    { app: 'pacenotes', note: 'Test plans' },
  ],
};
