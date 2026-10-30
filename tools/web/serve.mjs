// Static server for site/ that applies site/_headers the way Cloudflare Pages does (every rule
// whose path pattern matches adds its headers; `*` matches anything, `:name` one segment), so
// the page can be checked under its real CSP before a deploy. Local only; no game data. With
// `packs` (a module caller's) it answers /packs/* itself, in place of the packs the deploy puts
// there.
//
//   node tools/web/serve.mjs [--root site] [--port 8787] [--headers <root>/_headers]
//
// As a module: `const s = await serve({ root }); s.url; await s.close();`
import { createServer } from 'node:http';
import { readFile, realpath, stat } from 'node:fs/promises';
import { extname, isAbsolute, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.json': 'application/json',
  '.wasm': 'application/wasm',
  '.png': 'image/png',
  '.svg': 'image/svg+xml',
  '.ico': 'image/x-icon',
  '.txt': 'text/plain; charset=utf-8',
  '.woff2': 'font/woff2',
};

/// Parses a Pages `_headers` file into [{ pattern: RegExp, headers: [[name, value]] }].
export function parseHeaders(text) {
  const rules = [];
  let rule = null;
  for (const raw of text.split(/\r?\n/)) {
    if (raw.trim() === '' || raw.trimStart().startsWith('#')) continue;
    if (!/^\s/.test(raw)) {
      const path = raw.trim();
      const re = path
        .replace(/[.+?^${}()|[\]\\]/g, '\\$&')
        .replace(/\*/g, '.*')
        .replace(/:[A-Za-z]\w*/g, '[^/]+');
      rule = { path, pattern: new RegExp(`^${re}$`), headers: [] };
      rules.push(rule);
      continue;
    }
    if (!rule) throw new Error(`_headers: header line before any path: ${raw.trim()}`);
    const at = raw.indexOf(':');
    if (at < 0) throw new Error(`_headers: not a header line: ${raw.trim()}`);
    rule.headers.push([raw.slice(0, at).trim(), raw.slice(at + 1).trim()]);
  }
  return rules;
}

/// The headers `rules` give `path` (repeated names joined with ", ", as Pages does).
export function headersFor(rules, path) {
  const out = new Map();
  for (const rule of rules) {
    if (!rule.pattern.test(path)) continue;
    for (const [name, value] of rule.headers) {
      const key = name.toLowerCase();
      const prev = out.get(key);
      out.set(key, prev ? [prev[0], `${prev[1]}, ${value}`] : [name, value]);
    }
  }
  return [...out.values()];
}

async function loadRules(file) {
  try {
    return parseHeaders(await readFile(file, 'utf8'));
  } catch (err) {
    if (err.code === 'ENOENT') return [];
    throw err;
  }
}

/// Serves `root` on 127.0.0.1:`port` (0 = any free port). `packs`, when given, answers every
/// request under /packs/ in place of the files there: a function from the request (a fetch
/// Request) to its Response. Optional absolute `modelsRoot` mounts existing private model files
/// only under /models/, with normalized and realpath boundaries, on this loopback server.
export async function serve({ root = 'site', port = 0, headersFile, packs, modelsRoot } = {}) {
  const base = resolve(root);
  if (modelsRoot && !isAbsolute(modelsRoot)) throw new Error('modelsRoot must be an absolute local path');
  const modelsBase = modelsRoot ? await realpath(modelsRoot) : null;
  const rules = await loadRules(headersFile ?? join(base, '_headers'));
  let url = '';
  const server = createServer(async (req, res) => {
    try {
      const path = decodeURIComponent(new URL(req.url, 'http://local').pathname);
      const modelRequest = modelsBase && path.startsWith('/models/');
      const allowedBase = modelRequest ? modelsBase : base;
      let file = normalize(join(allowedBase, modelRequest ? path.slice('/models/'.length) : path));
      if (file !== allowedBase && !file.startsWith(allowedBase + sep)) throw Object.assign(new Error(), { code: 'ENOENT' });
      if (path === '/_headers') throw Object.assign(new Error(), { code: 'ENOENT' });
      if (packs && path.startsWith('/packs/')) {
        const headers = Object.entries(req.headers).filter(([, value]) => typeof value === 'string');
        const response = await packs(new Request(new URL(req.url, url), { method: req.method, headers }));
        const body = Buffer.from(await response.arrayBuffer());
        for (const [name, value] of headersFor(rules, path)) res.setHeader(name, value);
        response.headers.forEach((value, name) => res.setHeader(name, value));
        res.statusCode = response.status;
        res.end(body);
        return;
      }
      if ((await stat(file)).isDirectory()) file = join(file, 'index.html');
      if (modelRequest) {
        file = await realpath(file);
        if (file !== modelsBase && !file.startsWith(modelsBase + sep)) throw Object.assign(new Error(), { code: 'ENOENT' });
      }
      const body = await readFile(file);
      for (const [name, value] of headersFor(rules, path)) res.setHeader(name, value);
      res.setHeader('Content-Type', TYPES[extname(file)] ?? 'application/octet-stream');
      res.end(body);
    } catch (err) {
      res.statusCode = err.code === 'ENOENT' || err.code === 'ENOTDIR' ? 404 : 500;
      res.end(res.statusCode === 404 ? 'not found' : 'internal server error');
    }
  });
  await new Promise((done) => server.listen(port, '127.0.0.1', done));
  url = `http://127.0.0.1:${server.address().port}/`;
  return { url, rules, close: () => new Promise((done) => server.close(done)) };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const arg = (name, fallback) => {
    const i = process.argv.indexOf(`--${name}`);
    return i > 0 ? process.argv[i + 1] : fallback;
  };
  const s = await serve({ root: arg('root', 'site'), port: Number(arg('port', 8787)), headersFile: arg('headers') });
  console.log(`serving ${resolve(arg('root', 'site'))} at ${s.url} (${s.rules.length} header rule(s))`);
}
