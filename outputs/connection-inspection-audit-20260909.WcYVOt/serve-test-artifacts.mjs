import { createServer } from 'node:http';
import { createReadStream, statSync } from 'node:fs';
import { join } from 'node:path';

// Only two synthetic native test binaries, on the private VM bridge.
const artifactDirectory = process.argv[2];
if (!artifactDirectory) throw new Error('Pass the isolated native-test artifact directory');
const artifacts = new Map(['before', 'after'].map(name => [
  `/${name}.exe`, join(artifactDirectory, `windows-${name}-tests.exe`),
]));
createServer((request, response) => {
  const path = request.method === 'GET' && artifacts.get(request.url);
  if (!path) { response.writeHead(404).end(); return; }
  try {
    response.writeHead(200, { 'Content-Length': statSync(path).size });
    createReadStream(path).pipe(response);
  } catch { response.writeHead(404).end(); }
}).listen(18437, '192.168.64.1', () => console.log('QA artifact transfer ready'));
