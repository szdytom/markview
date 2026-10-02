// Dependency-free static server for the built demo site in `dist/`.
// `PORT` env selects the port (default 4173).

import { createServer } from "node:http";
import { existsSync } from "node:fs";
import { readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "dist");
const port = Number(process.env.PORT) || 4173;

if (!existsSync(path.join(root, "index.html"))) {
  console.error(
    [
      `serve.mjs: the built demo site is missing (no index.html under ${root}).`,
      "Build it first:",
      "  pnpm --dir web build",
    ].join("\n"),
  );
  process.exit(1);
}

const types = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".wasm": "application/wasm",
  ".otf": "font/otf",
  ".ttf": "font/ttf",
  ".json": "application/json",
  ".png": "image/png",
  ".svg": "image/svg+xml",
  ".ico": "image/x-icon",
};

createServer(async (req, res) => {
  try {
    const url = new URL(req.url, `http://${req.headers.host}`);
    let pathname = decodeURIComponent(url.pathname);
    if (pathname.endsWith("/")) pathname += "index.html";

    // Reject path traversal: the resolved path must stay inside `root`.
    const resolved = path.resolve(root, "." + pathname);
    if (resolved !== root && !resolved.startsWith(root + path.sep)) {
      res.writeHead(403).end("Forbidden");
      return;
    }

    const body = await readFile(resolved);
    res.writeHead(200, {
      "Content-Type": types[path.extname(resolved).toLowerCase()] ?? "application/octet-stream",
      "Cache-Control": "no-store",
    });
    res.end(body);
  } catch (err) {
    if (err?.code === "ENOENT") {
      res.writeHead(404).end("Not Found");
    } else if (err instanceof URIError) {
      res.writeHead(400).end("Bad Request");
    } else {
      console.error(`serve error: ${req.url}`, err);
      res.writeHead(500).end("Internal Server Error");
    }
  }
}).listen(port, "127.0.0.1", () => {
  console.log(`markview web demo on http://127.0.0.1:${port}/index.html`);
});
