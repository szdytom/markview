// Playwright config for the MVaaC acceptance harness.
// The suite drives the built demo site in `dist/` through `serve.mjs`.

import { defineConfig } from "@playwright/test";

// Headless Chromium falls back to SwiftShader unless ANGLE is pointed at the
// platform's Vulkan driver, so the default project rasterizes on the CPU. The
// `chromium-gpu` project asks for the real device; it is opt-in because a
// machine with no Vulkan driver would fail it rather than fall back.
const baseURL = `http://127.0.0.1:${process.env.PORT || 4173}`;
const gpuArgs = ["--use-angle=vulkan", "--enable-features=Vulkan"];

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false,
  reporter: "list",
  webServer: {
    command: "node serve.mjs",
    url: `${baseURL}/index.html`,
    reuseExistingServer: false,
    timeout: 30000,
  },
  use: {
    baseURL,
  },
  projects: [
    {
      name: "chromium",
      use: {
        browserName: "chromium",
        launchOptions: {
          args: ["--enable-unsafe-swiftshader"],
        },
      },
    },
    ...(process.env.MV_GPU
      ? [
          {
            name: "chromium-gpu",
            use: {
              browserName: "chromium",
              launchOptions: { args: gpuArgs },
            },
          },
        ]
      : []),
  ],
});
