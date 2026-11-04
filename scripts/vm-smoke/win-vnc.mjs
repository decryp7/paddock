// The VM's own VNC (enabled in the .vmx by run-win.mjs --vnc for one run, on the host's port 5995) through vncdotool: the only way to reach the
// UAC consent, which lives on the secure desktop where neither UI Automation nor SendInput from the session can. Keystrokes arrive as the
// virtual keyboard's, so they reach the secure desktop. The password is per run and is passed through the environment, never printed.
import { spawnSync } from 'node:child_process';
import { existsSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

const VENV = '/Volumes/d7sst7/dev-targets/tart-home/.paddock-vm-gui';
export const vncdo = () => {
  const dir = readdirSync(VENV).find((d) => d.startsWith('vnc-venv-'));
  const bin = dir && join(VENV, dir, 'bin', 'vncdo');
  if (!bin || !existsSync(bin)) throw new Error(`vncdotool is not installed under ${VENV}`);
  return bin;
};

export class Vnc {
  constructor({ port = 5995, password = process.env.PADDOCK_VNC_PW } = {}) { Object.assign(this, { port, password }); }

  run(args, timeoutMs = 60_000) {
    const r = spawnSync(vncdo(), ['-s', `127.0.0.1::${this.port}`, '-p', this.password ?? '', '--delay=60', ...args], { encoding: 'utf8', timeout: timeoutMs });
    return { code: r.status ?? 1, stderr: (r.stderr ?? '').replaceAll(this.password ?? '\0', '***') };
  }

  capture(file) { return this.run(['capture', file]); }
  /** Types text (the password goes through `type`, so it never appears in a log). */
  type(text) { return this.run(['type', text]); }
  key(k) { return this.run(['key', k]); }
}
