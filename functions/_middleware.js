// Cloudflare Pages Function in front of every path (site/_routes.json sends them all here). The
// HD packs under /packs/ are made from the game's files and are not public: a request whose
// canonical path is /packs or under /packs/ passes to the files only when its X-Pack-Key
// header is the key whose SHA-256 (hex) this file holds; with a wrong key or none it is a 404
// with an empty body, as for a file that is not there. Every other path goes on untouched.
// The canonical path is the one the static files could be found by, whatever the spelling: it
// is decoded until it no longer changes (a path that does not decode, or is not stable within
// MAX_DECODES decodes, is a 404 whatever it names), with \ as /, empty and . segments dropped,
// .. resolved, lowercased. Neither the key nor a pack is in the repository; the packs and their
// index.json are put into site/packs/ at deploy time only.

export const KEY_SHA256 = '637302b222111324a562643c3e206768a1e5bd8b3ee897d9f0695b3528c56922';
const MAX_DECODES = 5;

async function sha256Hex(text) {
  const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(text));
  return [...new Uint8Array(digest)].map((b) => b.toString(16).padStart(2, '0')).join('');
}

/// The canonical form of `pathname`, or null when it does not decode to one.
export function canonicalPath(pathname) {
  let path = pathname;
  for (let pass = 1; ; pass += 1) {
    let decoded;
    try {
      decoded = decodeURIComponent(path);
    } catch {
      return null;
    }
    if (decoded === path) break;
    if (pass === MAX_DECODES) return null;
    path = decoded;
  }
  const segments = [];
  for (const segment of path.replace(/\\/g, '/').split('/')) {
    if (segment === '..') segments.pop();
    else if (segment !== '' && segment !== '.') segments.push(segment);
  }
  return `/${segments.join('/')}`.toLowerCase();
}

/// The middleware for the key whose SHA-256 is `sha256` (the tests pass a key of their own).
export function gate(sha256) {
  return async (context) => {
    const path = canonicalPath(new URL(context.request.url).pathname);
    if (path === null) return new Response(null, { status: 404 });
    if (path !== '/packs' && !path.startsWith('/packs/')) return context.next();
    const key = context.request.headers.get('X-Pack-Key');
    if (key !== null && (await sha256Hex(key)) === sha256) return context.next();
    return new Response(null, { status: 404 });
  };
}

export const onRequest = gate(KEY_SHA256);
