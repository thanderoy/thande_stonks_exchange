#!/usr/bin/env node
// Prints the CSP source list for every inline <script> in the built site,
// e.g. `'sha256-abc=' 'sha256-def='`.
//
// Qwik's SSG output contains a few small inline bootstrap scripts whose
// content changes with each build (they reference hashed bundle names), so
// the hashes are computed after `npm run build` and handed to Caddy, which
// puts them in the Content-Security-Policy header. This keeps `script-src`
// free of 'unsafe-inline'.
//
// Usage: node scripts/csp-hashes.mjs [dist-dir] > csp-script-hashes
import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { join } from "node:path";

const root = process.argv[2] ?? "dist";

async function* htmlFiles(dir) {
  for (const entry of await readdir(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) yield* htmlFiles(path);
    else if (entry.name.endsWith(".html")) yield path;
  }
}

// Inline scripts the browser executes: no src, and no type or a JS type.
// `type="qwik/json"` is data and is not subject to script-src.
const SCRIPT_RE = /<script(\s[^>]*)?>([\s\S]*?)<\/script>/gi;
const JS_TYPES = new Set([
  "",
  "module",
  "text/javascript",
  "application/javascript",
]);

function isExecutableInline(attrs) {
  if (/\ssrc\s*=/i.test(attrs)) return false;
  const type = /\stype\s*=\s*["']?([^"'\s>]*)/i.exec(attrs)?.[1] ?? "";
  return JS_TYPES.has(type.toLowerCase());
}

const hashes = new Set();
let files = 0;
for await (const file of htmlFiles(root)) {
  files += 1;
  const html = await readFile(file, "utf8");
  for (const match of html.matchAll(SCRIPT_RE)) {
    const attrs = match[1] ?? "";
    if (!isExecutableInline(attrs)) continue;
    const digest = createHash("sha256")
      .update(match[2], "utf8")
      .digest("base64");
    hashes.add(`'sha256-${digest}'`);
  }
}

if (files === 0) {
  console.error(`csp-hashes: no HTML files under ${root}; run the build first`);
  process.exit(1);
}

process.stdout.write([...hashes].sort().join(" "));
console.error(
  `csp-hashes: ${hashes.size} inline script hashes from ${files} HTML files`,
);
