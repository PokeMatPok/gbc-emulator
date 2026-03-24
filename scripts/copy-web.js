/**
 * Copies static web assets into the web/ directory after wasm-pack build.
 * wasm-pack already outputs into web/pkg/; this just validates the output.
 */

import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const PKG = path.resolve(__dirname, '../web/pkg');

const required = ['gbc_emulator.js', 'gbc_emulator_bg.wasm'];
let ok = true;

for (const f of required) {
  const p = path.join(PKG, f);
  if (fs.existsSync(p)) {
    console.log(`✓ ${f}`);
  } else {
    console.error(`✗ Missing: ${f} (run wasm-pack build first)`);
    ok = false;
  }
}

if (!ok) process.exit(1);
console.log('Web assets ready in web/pkg/');
