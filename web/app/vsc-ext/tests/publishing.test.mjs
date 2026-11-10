import { test } from "node:test";
import assert from "node:assert/strict";
import { getOIDCCredential } from "@vscode/vsce/out/oidc.js";

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
