// The local HTTPS update feed of the scripted update tests (macOS: run.mjs --update-test; Windows: update-win.mjs). Bound to one VM
// bridge address only, with the test CA's server certificate; `/ok/latest.json` is always the CURRENT case's manifest (the old build has
// that one URL built in), the other files are the current case's folder. The case `down` resets every connection.
import { createReadStream, existsSync, readFileSync, statSync } from 'node:fs';
import { createServer } from 'node:https';
import { join } from 'node:path';
import { feedRoute } from './update-judge.mjs';

/** @param {{ assets: string, host: string, port: number }} o  assets: the folder with feed/ and ca/ (spotter-update-assets*.sh) */
export async function startFeed({ assets, host, port }) {
  const root = join(assets, 'feed');
  let current = 'ok';
  let log = [];
  const server = createServer({ key: readFileSync(join(assets, 'ca', 'server.key')), cert: readFileSync(join(assets, 'ca', 'server.pem')) }, (req, res) => {
    if (current === 'down') { req.socket.destroy(); return; }
    const route = feedRoute(req.url ?? '/', current);
    log.push(`${req.method} ${req.url} ${route ? 200 : 404}`);
    if (!route || !existsSync(join(root, route.file))) { res.writeHead(404).end(); return; }
    res.writeHead(200, { 'content-type': route.type, 'content-length': statSync(join(root, route.file)).size });
    createReadStream(join(root, route.file)).pipe(res);
  });
  await new Promise((r, j) => { server.once('error', j); server.listen(port, host, r); });
  return { setCase: (id) => { current = id; }, feedLog: () => log, clearFeedLog: () => { log = []; }, close: () => server.close() };
}
