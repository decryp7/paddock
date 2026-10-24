// Colour tokens for diagrams: the roles of STYLE.md, per app. Light values are the Paddock light tokens, dark values the dark ones.
// Tints (fills) are derived: 10 % of the stroke over white in light, 16 % over the dark surface (#15171B) in dark.

const mix = (hex, bg, a) => {
  const p = (h) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16));
  const [c, b] = [p(hex), p(bg)];
  return '#' + c.map((v, i) => Math.round(b[i] + (v - b[i]) * a).toString(16).padStart(2, '0')).join('').toUpperCase();
};
const role = (l, d) => ({ s: [l, d], f: [mix(l, '#FFFFFF', 0.1), mix(d, '#15171B', 0.16)] });

// Roles that do not depend on the app.
export const FIXED = {
  plugin: role('#B0508A', '#D98BC0'),
  process: { s: ['#565E6B', '#A0A7B2'], f: ['#F0F2F5', '#1B1E23'] },
  external: { s: ['#6B7380', '#7F8794'], f: ['#F4F5F7', '#17191E'] },
  group: { s: ['#C4C7CD', '#353A42'], f: ['#F8F9FA', '#121418'] },
  text: ['#14171C', '#ECEEF1'],
  muted: ['#565E6B', '#A0A7B2'],
  edge: ['#565E6B', '#A0A7B2'],
  white: ['#FFFFFF', '#15171B'],
};

const GREEN = ['#007A5E', '#2EC4A0'];
const INDIGO = ['#4C5BD4', '#8FA3FF'];
const SKY = ['#0072B2', '#56B4E9'];
const VERM = ['#C24E00', '#F07D3A'];

// accent: the app's own containers. core: pure library. ui: window / webview.
export const APPS = {
  kit: { name: 'Paddock kit', accent: ['#0E7490', '#22D3EE'], core: GREEN, ui: INDIGO },
  debrief: { name: 'Debrief', accent: ['#1D5FD1', '#5B9BFF'], core: GREEN, ui: VERM },
  pitcrew: { name: 'Pitcrew', accent: ['#BF3A0C', '#FF7A45'], core: GREEN, ui: INDIGO },
  pitwall: { name: 'Pitwall', accent: ['#6D3FD6', '#A58BFA'], core: GREEN, ui: SKY },
  dyno: { name: 'Dyno', accent: ['#B01E6E', '#F063A8'], core: GREEN, ui: INDIGO },
  sighter: { name: 'Sighter', accent: ['#884F92', '#C488CF'], core: GREEN, ui: INDIGO },
  pacenotes: { name: 'Pacenotes', accent: ['#576D07', '#8DA74E'], core: INDIGO, ui: SKY },
  spotter: { name: 'Spotter', accent: ['#0B7A6E', '#2CC4B0'], core: INDIGO, ui: SKY },
};

export function theme(app = 'kit') {
  const a = APPS[app];
  if (!a) throw new Error(`unknown app "${app}"; one of ${Object.keys(APPS).join(', ')}`);
  return {
    app: a.name,
    accent: role(...a.accent),
    core: role(...a.core),
    ui: role(...a.ui),
    ...FIXED,
  };
}
