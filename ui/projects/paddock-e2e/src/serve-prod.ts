// Serves an app's production build (dist/app/browser) with the exact Content-Security-Policy the Tauri webview
// enforces, read from src-tauri/tauri.conf.json so the two can never drift. Shared by the apps' Playwright configs
// (rule 42): a static bundle has no per-module compile or request latency, so a slow module cannot stall a page
// (ER-12). Usage: node serve-prod.ts <app dir> <port>
import { createReadStream, readFileSync, statSync } from 'node:fs';
import { createServer, type Server } from 'node:http';
import { extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const TYPES: Readonly<Record<string, string>> = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.ico': 'image/x-icon',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.txt': 'text/plain; charset=utf-8',
};

/** The Tauri CSP of the app in `appDir`; throws when it has none (never serve without the production policy). */
export function tauriCsp(appDir: string): string {
  const conf = JSON.parse(readFileSync(join(appDir, 'src-tauri', 'tauri.conf.json'), 'utf8'));
  const csp = conf?.app?.security?.csp;
  if (typeof csp !== 'string' || !csp.trim()) throw new Error('tauri.conf.json has no app.security.csp string; refusing to serve without the production CSP.');
  return csp;
}

/** The file a URL path names under `root`, the SPA entry point for an extensionless path, or null (404, or outside root). */
export function resolveFile(root: string, urlPath: string): string | null {
  let decoded: string;
  try {
    decoded = decodeURIComponent(urlPath.split('?')[0] ?? '');
  } catch {
    return null;
  }
  const candidate = normalize(join(root, decoded));
  if (candidate !== root && !candidate.startsWith(root + sep)) return null;
  try {
    if (statSync(candidate).isFile()) return candidate;
  } catch {
    /* fall through to the SPA entry point */
  }
  return extname(decoded) ? null : join(root, 'index.html');
}

/** Starts the static server on `port` for the build in `<appDir>/dist/app/browser`. */
export function serveProd(appDir: string, port: number): Server {
  const root = join(resolve(appDir), 'dist', 'app', 'browser');
  const csp = tauriCsp(appDir);
  return createServer((req, res) => {
    const file = resolveFile(root, req.url ?? '/');
    if (!file) {
      res.writeHead(404, { 'Content-Security-Policy': csp }).end('Not found');
      return;
    }
    res.writeHead(200, {
      'Content-Type': TYPES[extname(file)] ?? 'application/octet-stream',
      'Content-Security-Policy': csp,
      'Cache-Control': 'no-store',
    });
    createReadStream(file).pipe(res);
  }).listen(port, () => console.log(`prod build with Tauri CSP on http://localhost:${port}\nCSP: ${csp}`));
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [appDir, port] = process.argv.slice(2);
  if (!appDir || !Number.isInteger(Number(port))) {
    console.error('::error::usage: node serve-prod.ts <app dir> <port>');
    process.exit(2);
  }
  try {
    serveProd(appDir, Number(port));
  } catch (e) {
    console.error(`::error::${e instanceof Error ? e.message : String(e)} Next step: run it from an app that has src-tauri/tauri.conf.json.`);
    process.exit(1);
  }
}
