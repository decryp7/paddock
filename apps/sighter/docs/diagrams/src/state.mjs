import { P, save, link, dot } from './common.mjs';

const d = P({ title: 'Element label lifecycle', kind: 'UML · State machine · planned', w: 1552,
  desc: 'A label starts as a suggestion, is reviewed by a person, trains a model and, when the UI changes or confidence drops, returns to review.' });
const W = 152, H = 120, X0 = 112, G = 104, cx = (i) => X0 + i * (W + G), Y = 168, bus = Y + H + 56;
const st = (id, i, name, desc) => d.node(id, cx(i), Y, W, H, 'accent', { name, desc });
dot(d, 'ini', 48, Y + H / 2 - 8, 'start');
st('sug', 0, 'Suggested', 'OCR, template, detector or Pitwall proposed a box.');
st('need', 1, 'NeedsLabelling', 'In the review queue; also anything nothing identified.');
st('rev', 2, 'Reviewed', 'A person accepted, fixed or drew it.');
st('set', 3, 'InTrainingSet', 'Split by session; reviewed labels only.');
st('mod', 4, 'InModel', 'In a trained model app@N, immutable.');
st('low', 5, 'LowConfidence', 'Low confidence or a UI change at run time.');
d.edge('ini', 'sug', d.port('ini', 'R'), d.port('sug', 'L'));
link(d, 'sug', 'R', 'need', 'L', { label: 'Queued' });
link(d, 'need', 'R', 'rev', 'L', { label: 'Review' });
link(d, 'rev', 'R', 'set', 'L', { label: 'Included' });
link(d, 'set', 'R', 'mod', 'L', { label: 'Trained' });
link(d, 'mod', 'R', 'low', 'L', { label: 'UI changed' });
const a = d.port('low', 'B', 0.5), b = d.port('need', 'B', 0.5);
d.edge('low', 'need', a, b, { via: [[a[0], bus], [b[0], bus]] });
d.lbl('Flagged: back to review', (a[0] + b[0]) / 2, bus, 'b');
d.legend([{ kind: 'accent', label: 'Label state' }, { kind: 'line', label: 'Transition' }]);
save('uml-state-element-label', d, bus + 40);
