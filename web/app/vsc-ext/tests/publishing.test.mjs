import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { getOIDCCredential } from "@vscode/vsce/out/oidc.js";
import { listFiles } from "@vscode/vsce/out/package.js";

test("installed vsce preserves the package allowlist with the patched glob matcher", async (t) => {
  const cwd = await mkdtemp(path.join(tmpdir(), "markview-vsce-"));
  t.after(() => rm(cwd, { recursive: true, force: true }));
  const included = [
    "package.json",
    "README.md",
    "LICENSE",
    "CHANGELOG.md",
    "icon.png",
    "dist/extension.cjs",
    "media/nested/viewer.js",
    "media/.hidden.css",
    "bin/markview",
  ];
  const excluded = ["src/extension.ts", "dist/extension.cjs.map", ".env"];
  await Promise.all(
    [...included, ...excluded].map(async (file) => {
      const target = path.join(cwd, file);
      await mkdir(path.dirname(target), { recursive: true });
      await writeFile(target, file === "package.json" ? "{}" : "fixture");
    }),
  );
  await writeFile(
    path.join(cwd, ".vscodeignore"),
    await readFile(new URL("../vscode/.vscodeignore", import.meta.url)),
  );
  const files = await listFiles({ cwd, dependencies: false, manifest: {} });
  assert.deepEqual(files.sort(), included.sort());
});

test("installed vsce exchanges GitHub OIDC tokens using the Marketplace contract", async () => {
  const requests = [];
  const credential = await getOIDCCredential("Stevvven", {
    environment: {
      GITHUB_ACTIONS: "true",
      ACTIONS_ID_TOKEN_REQUEST_URL: "https://github.example/token",
      ACTIONS_ID_TOKEN_REQUEST_TOKEN: "github-request-token",
    },
    marketplaceUrl: "https://marketplace.visualstudio.com",
    request: async (url, request) => {
      requests.push({ url: new URL(url), ...request });
      return {
        statusCode: 200,
        readBody: async () =>
          JSON.stringify(
            request.method === "GET"
              ? { value: "github-oidc-token" }
              : { credential: "marketplace-session-token" },
          ),
      };
    },
  });

  assert.equal(credential, "marketplace-session-token");
  assert.equal(requests.length, 2);
  const [github, marketplace] = requests;
  assert.equal(
    github.url.searchParams.get("audience"),
    "marketplace.visualstudio.com",
  );
  assert.equal(github.headers.Authorization, "Bearer github-request-token");
  assert.equal(marketplace.method, "POST");
  assert.equal(marketplace.url.pathname, "/_apis/gallery/token");
  assert.equal(
    marketplace.url.searchParams.get("api-version"),
    "7.2-preview.1",
  );
  assert.equal(
    marketplace.headers.Authorization,
    "FederatedToken github-oidc-token",
  );
  assert.deepEqual(JSON.parse(marketplace.body), { publisherName: "Stevvven" });
});
