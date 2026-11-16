// Builds every UI cell bundle (one self-contained script each; its sha256 is what the manifest entry pins and
// what the frame's CSP allows), the page, and the sim worker, into web/dist.
//   node build.mjs            everything
//   node build.mjs --only notes   only the bundles of cells/notes/ui
//   node build.mjs --pages    as published on GitHub Pages: the ops.panel bundle's bytes are left out
import { build } from 'esbuild';
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';

const args = process.argv.slice(2);
const pages = args.includes('--pages');
const only = args.includes('--only') ? args[args.indexOf('--only') + 1] : null;
const plan = JSON.parse(readFileSync('plan.json', 'utf8'));
mkdirSync('dist/bundles', { recursive: true });
const index = existsSync('dist/bundles.json') && only ? JSON.parse(readFileSync('dist/bundles.json', 'utf8')) : {};

for (const b of plan.bundles) {
  if (only && b.dir !== only) continue;
  const r = await build({ entryPoints: [b.entry], bundle: true, format: 'iife', minify: true, write: false, target: 'es2022', legalComments: 'none', charset: 'utf8' });
  const bytes = r.outputFiles[0].contents;
  const sha = createHash('sha256').update(bytes).digest('hex');
  const file = `bundles/${b.view}@${b.version}.js`;
  const pub = !(pages && b.private);
  if (pub) writeFileSync('dist/' + file, bytes);
  else rmSync('dist/' + file, { force: true });
  index[`${b.view}@${b.version}`] = { file: pub ? file : null, sha, bytes: bytes.length, dir: b.dir };
  console.log(`${file}  ${bytes.length} bytes  sha256 ${sha.slice(0, 16)}${pub ? '' : '  (bytes not published: only its hash)'}`);
}
writeFileSync('dist/bundles.json', JSON.stringify(index, null, 1));
if (!only) {
  await build({ entryPoints: ['page/main.ts'], bundle: true, format: 'esm', minify: true, outfile: 'dist/page.js', target: 'es2022', legalComments: 'none' });
  await build({ entryPoints: ['sim/worker.ts'], bundle: true, format: 'iife', minify: true, outfile: 'dist/worker.js', target: 'es2022', legalComments: 'none' });
  for (const f of ['index.html', 'page.css']) cpSync('page/' + f, 'dist/' + f);
  cpSync('cell.css', 'dist/cell.css');
  cpSync('plan.json', 'dist/plan.json');
  mkdirSync('dist/kit/fonts', { recursive: true });
  for (const f of ['tokens.css', 'components.css']) cpSync('vendor/ui-cells/kit/' + f, 'dist/kit/' + f);
  cpSync('vendor/ui-cells/kit/fonts', 'dist/kit/fonts', { recursive: true });
  if (existsSync('../proof/summary.json')) {
    rmSync('dist/proof', { recursive: true, force: true });
    cpSync('../proof', 'dist/proof', { recursive: true });
  }
  console.log('dist/page.js dist/worker.js dist/index.html');
}
