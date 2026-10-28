// Playwright globalSetup of the apps: they consume ui/dist, so ui/dist and the generated css must be fresh.
const { join } = require('node:path');
const { assertFreshBuilds, REPO_ROOT } = require('./e2e-guard.cjs');
module.exports = function globalSetup() {
  assertFreshBuilds(join(REPO_ROOT, 'ui'), { checkDist: true });
};
