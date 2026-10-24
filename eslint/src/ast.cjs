'use strict';
// Small AST helpers shared by the rules.

/** Calls `visit(node, parent)` for `node` and every descendant, using the parser's visitor keys. */
function walk(node, visitorKeys, visit, parent = null) {
  visit(node, parent);
  const keys = visitorKeys[node.type] ?? [];
  for (const key of keys) {
    const child = node[key];
    if (Array.isArray(child)) {
      for (const c of child) if (c && typeof c.type === 'string') walk(c, visitorKeys, visit, node);
    } else if (child && typeof child.type === 'string') walk(child, visitorKeys, visit, node);
  }
}

const CONTAINERS = new Set(['Program', 'BlockStatement', 'ClassBody', 'StaticBlock', 'TSModuleBlock']);

/** The statement-like node that holds `node`: the climb stops below a block, class body or program. */
function statementOf(node) {
  let n = node;
  while (n.parent && !CONTAINERS.has(n.parent.type)) n = n.parent;
  return n;
}

/** True when a comment right before `node`'s statement contains `tag`. */
function hasTagBefore(sourceCode, node, tag) {
  return sourceCode.getCommentsBefore(statementOf(node)).some((c) => c.value.includes(tag));
}

/** Name of a property key (identifier or string literal), or null. */
function keyName(prop) {
  if (!prop || prop.type !== 'Property' || prop.computed) return null;
  if (prop.key.type === 'Identifier') return prop.key.name;
  if (prop.key.type === 'Literal') return String(prop.key.value);
  return null;
}

/** The property named `name` of an ObjectExpression, or undefined. */
function prop(obj, name) {
  return obj?.type === 'ObjectExpression' ? obj.properties.find((p) => keyName(p) === name) : undefined;
}

module.exports = { walk, statementOf, hasTagBefore, keyName, prop };
