// A stand-in for Gitea's generic package API, for tests/run.sh only: PUT stores a file once (201, then 409), GET returns
// it (404 when missing), and every request must carry the test token. Prints its port, then serves until killed.
import { createServer } from 'node:http';

const files = new Map();
const server = createServer((req, res) => {
  if (req.headers.authorization !== 'token test-token') {
    res.writeHead(401).end('no token');
    return;
  }
  if (req.method === 'GET') {
    const f = files.get(req.url);
    if (f) res.writeHead(200).end(f);
    else res.writeHead(404).end('not found');
    return;
  }
  if (req.method !== 'PUT') {
    res.writeHead(405).end();
    return;
  }
  const parts = [];
  req.on('data', (c) => parts.push(c));
  req.on('end', () => {
    if (files.has(req.url)) {
      res.writeHead(409).end('package file already exists');
      return;
    }
    files.set(req.url, Buffer.concat(parts));
    res.writeHead(201).end();
  });
});
server.listen(0, '127.0.0.1', () => console.log(server.address().port));
