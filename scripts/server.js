/**
 * Minimal static file server for local development.
 * Serves the web/ directory with correct MIME types and COOP/COEP headers
 * required for SharedArrayBuffer (used by WebAssembly).
 */

import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const WEB_DIR = path.resolve(__dirname, '../web');
const PORT = process.env.PORT ?? 8080;

const MIME = {
  '.html': 'text/html',
  '.css':  'text/css',
  '.js':   'application/javascript',
  '.mjs':  'application/javascript',
  '.wasm': 'application/wasm',
  '.ico':  'image/x-icon',
  '.png':  'image/png',
  '.gb':   'application/octet-stream',
  '.gbc':  'application/octet-stream',
};

const server = http.createServer((req, res) => {
  let url = req.url === '/' ? '/index.html' : req.url;
  // Strip query strings
  url = url.split('?')[0];

  const filePath = path.join(WEB_DIR, url);

  // Basic path traversal protection
  if (!filePath.startsWith(WEB_DIR)) {
    res.writeHead(403); res.end('Forbidden'); return;
  }

  fs.readFile(filePath, (err, data) => {
    if (err) {
      res.writeHead(404, { 'Content-Type': 'text/plain' });
      res.end('Not found: ' + url);
      return;
    }

    const ext = path.extname(filePath).toLowerCase();
    const mime = MIME[ext] ?? 'application/octet-stream';

    res.writeHead(200, {
      'Content-Type': mime,
      // Required for SharedArrayBuffer / high-resolution timer
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
      'Cache-Control': 'no-cache',
    });
    res.end(data);
  });
});

server.listen(PORT, () => {
  console.log(`GBC Emulator dev server: http://localhost:${PORT}`);
  console.log(`Serving: ${WEB_DIR}`);
});
