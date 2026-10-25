import assert from 'node:assert/strict';
import { test } from 'node:test';
import { checkCapabilities, checkConfig, checkCsp, cspText } from './check-capabilities.mjs';

const good = { identifier: 'default', permissions: ['core:event:default', 'core:window:allow-start-dragging', { identifier: 'core:window:allow-internal-toggle-maximize' }] };
const csp = "default-src 'self' ipc: http://ipc.localhost; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost; object-src 'none'";

test('a minimal capability passes', () => {
  assert.deepEqual(checkCapabilities([good]), []);
});

test('wildcards, fs and shell grants are refused', () => {
  const bad = { identifier: 'x', permissions: ['fs:default', 'shell:allow-execute', 'core:*'] };
  assert.equal(checkCapabilities([good, bad]).length, 3);
});

test('remote content capabilities are refused', () => {
  assert.equal(checkCapabilities([good, { identifier: 'r', remote: { urls: ['https://x'] }, permissions: [] }]).length, 1);
});

test('missing drag permissions are reported', () => {
  assert.equal(checkCapabilities([{ identifier: 'a', permissions: ['core:event:default'] }]).length, 2);
});

test('the CSP rules', () => {
  assert.deepEqual(checkCsp(csp), []);
  assert.equal(checkCsp(csp + "; script-src 'self' 'unsafe-eval'").length, 1);
  assert.equal(checkCsp(csp + '; img-src https://example.com').length, 1);
  assert.equal(checkCsp("script-src 'self' 'unsafe-inline'").length, 1);
  assert.equal(checkCsp('').length, 1);
});

test('a CSP object is joined into a string', () => {
  assert.equal(cspText({ 'default-src': ["'self'", 'ipc:'] }), "default-src 'self' ipc:");
});

test('config checks private API and the ad-hoc seal', () => {
  const conf = { app: { macOSPrivateApi: true, security: { csp } }, bundle: { macOS: { signingIdentity: '-' } } };
  assert.deepEqual(checkConfig(conf), []);
  assert.deepEqual(checkConfig({ app: conf.app }, { bundle: { macOS: { signingIdentity: '-' } } }), []);
  assert.equal(checkConfig({ app: { security: { csp } } }).length, 2);
});
