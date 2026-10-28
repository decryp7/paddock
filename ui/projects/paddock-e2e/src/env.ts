/** The first port of the range the local defaults are derived into. */
export const PORT_RANGE_START = 20_000;
/** The number of worktree blocks; each block holds 100 ports (the default's last two digits pick the port in it). */
export const PORT_BLOCKS = 400;

/** A stable 32-bit FNV-1a hash of a string. */
export function hashString(text: string): number {
  let h = 0x811c9dc5;
  for (const ch of Buffer.from(text, 'utf8')) {
    h ^= ch;
    h = Math.imul(h, 0x01000193) >>> 0;
  }
  return h;
}

/**
 * The local default port of a config whose usual port is `fallback` (4300, 4352, ...): a block of 100 ports picked by a
 * hash of `root` (the folder the run starts in, so a worktree), plus the last two digits of `fallback`. Two worktrees
 * land in different blocks unless the hashes collide (1 in 400), and the ports of one worktree never overlap.
 */
export function worktreePort(fallback: number, root: string): number {
  return PORT_RANGE_START + (hashString(root) % PORT_BLOCKS) * 100 + (fallback % 100);
}

/**
 * The e2e port of a worktree or CI job (rule 2: each agent its own port). Read from the environment variable `name`;
 * locally, when it is unset, the port is derived from the worktree ({@link worktreePort}, so two worktrees do not collide
 * by default), on CI (`CI` set) a missing port is an error, because a default could collide with another job on the same
 * runner host.
 */
export function e2ePort(name: string, fallback: number, env: Readonly<Record<string, string | undefined>> = process.env, root: string = process.cwd()): number {
  return portFrom(name, fallback, env, root, false);
}

/**
 * Like {@link e2ePort} for the apps' own configs, which have always run on CI with fixed ports: on CI an unset variable gives
 * `fallback` itself (deterministic), locally the worktree-derived port.
 */
export function e2ePortOrCiDefault(name: string, fallback: number, env: Readonly<Record<string, string | undefined>> = process.env, root: string = process.cwd()): number {
  return portFrom(name, fallback, env, root, true);
}

function portFrom(name: string, fallback: number, env: Readonly<Record<string, string | undefined>>, root: string, ciDefault: boolean): number {
  const raw = env[name];
  if (raw === undefined || raw === '') {
    if (env['CI']) {
      if (ciDefault) return fallback;
      throw new Error(`CI: set ${name} (the e2e server's port); a fixed default could collide with another job`);
    }
    return worktreePort(fallback, root);
  }
  const port = Number(raw);
  if (!Number.isInteger(port) || port < 1 || port > 65_535) throw new Error(`${name} is not a port: ${JSON.stringify(raw)}`);
  return port;
}

/** Whether a variable is switched on (set to anything but empty or `0`): the one test for CI, PW_REUSE_SERVER and PW_SKIP_BUILD_GUARD. */
export function isOn(value: string | undefined): boolean {
  return value !== undefined && value !== '' && value !== '0';
}
