'use strict';
// ADR 0018 / design guide (Debrief's lint-styles.mjs as a rule): component styles read theme tokens (`var(--...)`), never
// hard-coded colours, font families or font sizes. Checks the `styles` of `@Component` and every template literal of a
// `*-styles.ts` module. Unknown `var(--x)` names need the project's token list and stay in each app's own check.
const { keyName } = require('../ast.cjs');

const NAMED = 'white|black|red|green|blue|yellow|orange|purple|gray|grey|pink|silver|navy|teal|maroon|olive|lime|aqua|fuchsia|magenta|cyan|brown|gold|indigo|violet';
const RULES = [
  { id: 'hex colour', re: /(^|[\s:,(])#[0-9a-fA-F]{3,8}(?![\w-])/ },
  { id: 'colour function', re: /\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch)\(/ },
  { id: 'named colour', re: new RegExp(`:\\s*[^;{}]*\\b(${NAMED})\\b`, 'i') },
  { id: 'font size', re: /font-size\s*:\s*-?[\d.]+(px|pt|rem)\b/ },
  { id: 'font family', re: /font-family\s*:\s*(?!\s*(var\(--|inherit\b))/ },
  { id: 'font shorthand', re: /(^|[\s;{])font\s*:[^;]*(?<![/\d.])[\d.]+(px|pt|rem)\b/ },
];

const stripComments = (css) => css.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' '));

/** The problems of one block of CSS: [{ line (1-based in the block), rule, text }]. Exported for tests. */
function cssProblems(css) {
  const out = [];
  stripComments(css).split('\n').forEach((text, i) => {
    for (const r of RULES) if (r.re.test(text)) out.push({ line: i + 1, rule: r.id, text: text.trim() });
  });
  return out;
}

/** @type {import('eslint').Rule.RuleModule} */
module.exports = {
  meta: {
    type: 'problem',
    docs: { description: 'Component styles use theme tokens, not hard-coded colours, font families or sizes.' },
    schema: [],
    messages: { raw: 'Hard-coded {{rule}} in styles: `{{text}}`. Use a theme token, var(--...).' },
  },
  create(context) {
    const isStyleModule = /-styles\.[cm]?ts$/.test(context.filename);
    const checkLiteral = (node) => {
      if (node.type === 'Literal' && typeof node.value === 'string') return scan(node, node.value);
      if (node.type === 'TemplateLiteral') return scan(node, node.quasis.map((q) => q.value.cooked ?? q.value.raw).join(' '));
      if (node.type === 'ArrayExpression') node.elements.forEach((e) => e && checkLiteral(e));
    };
    const scan = (node, css) => {
      for (const p of cssProblems(css)) context.report({ node, loc: { line: node.loc.start.line + p.line - 1, column: 0 }, messageId: 'raw', data: { rule: p.rule, text: p.text } });
    };
    return {
      Property(node) {
        if (keyName(node) !== 'styles') return;
        const call = node.parent?.parent;
        if (call?.type === 'CallExpression' && call.callee.type === 'Identifier' && call.callee.name === 'Component') checkLiteral(node.value);
      },
      TemplateLiteral(node) {
        if (isStyleModule) scan(node, node.quasis.map((q) => q.value.cooked ?? q.value.raw).join(' '));
      },
    };
  },
  cssProblems,
};
