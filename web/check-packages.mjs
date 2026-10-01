// Install actual package tarballs into an isolated downstream consumer.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import esbuild from "esbuild";
import { chromium } from "@playwright/test";

const root = dirname(fileURLToPath(import.meta.url));
const consumer = mkdtempSync(join(tmpdir(), "markview-packages-"));
let browser;
try {
	const names = ["viewer", "editor", "resources", "web"];
	const dependencies = {};
	for (const name of names) {
		const pkg = name === "viewer" ? "markview" : name;
		const { version } = JSON.parse(
			readFileSync(join(root, "packages", pkg, "package.json"), "utf8"),
		);
		execFileSync("pnpm", ["pack", "--pack-destination", consumer], {
			cwd: join(root, "packages", pkg),
			stdio: "pipe",
		});
		dependencies[`@markview/${name}`] =
			`file:${join(consumer, `markview-${name}-${version}.tgz`)}`;
	}
	writeFileSync(
		join(consumer, "package.json"),
		JSON.stringify({
			name: "markview-consumer",
			private: true,
			type: "module",
			dependencies,
		}),
	);
	writeFileSync(
		join(consumer, "pnpm-workspace.yaml"),
		"overrides:\n" +
			Object.entries(dependencies)
				.map(
					([name, path]) =>
						`  ${JSON.stringify(name)}: ${JSON.stringify(path)}\n`,
				)
				.join(""),
	);
	execFileSync("pnpm", ["install", "--ignore-scripts"], {
		cwd: consumer,
		stdio: "pipe",
	});
	const main = `
import { Editor } from "@markview/editor";
import { Viewer, init, type SourceGeometry } from "@markview/viewer";
import { browserResources, decodeImage } from "@markview/resources";
import { CanvasReader } from "@markview/web";
await init({wasmUrl:"/viewer.wasm",fonts:["/body.otf"]});
const editor = await Editor.mount(document.body,{markdown:"# Hello",viewer:{resources:browserResources()}});
const geometry: SourceGeometry | null = editor.viewer.sourceToPreview(0);
console.log(geometry, Viewer, CanvasReader, decodeImage);
editor.destroy();
Object.assign(window,{packageSmoke:true});
`;
	writeFileSync(join(consumer, "main.ts"), main);
	execFileSync(
		process.execPath,
		[
			join(root, "node_modules/typescript/bin/tsc"),
			"--noEmit",
			"--strict",
			"--skipLibCheck",
			"false",
			"--target",
			"es2022",
			"--module",
			"esnext",
			"--moduleResolution",
			"bundler",
			"main.ts",
		],
		{ cwd: consumer, stdio: "inherit" },
	);
	const result = await esbuild.build({
		absWorkingDir: consumer,
		entryPoints: ["main.ts"],
		bundle: true,
		format: "esm",
		write: false,
	});
	browser = await chromium.launch({ args: ["--enable-unsafe-swiftshader"] });
	const page = await browser.newPage();
	const errors = [];
	page.on("pageerror", (error) => errors.push(String(error)));
	await page.route("**/*", (route) => {
		const path = new URL(route.request().url()).pathname;
		if (path === "/")
			return route.fulfill({
				contentType: "text/html",
				body: '<body style="height:600px;width:900px;margin:0"><script type="module" src="/main.js"></script>',
			});
		if (path === "/main.js")
			return route.fulfill({
				contentType: "text/javascript",
				body: result.outputFiles[0].text,
			});
		if (path === "/viewer.wasm")
			return route.fulfill({
				contentType: "application/wasm",
				path: join(
					consumer,
					"node_modules/@markview/viewer/dist/markview_web_bg.wasm",
				),
			});
		if (path === "/body.otf")
			return route.fulfill({
				contentType: "font/otf",
				body: readFileSync(
					join(
						root,
						"../crates/markview-core/tests/fonts/NotoSerif-Regular-subset.otf",
					),
				),
			});
		return route.abort();
	});
	await page.goto("http://markview-package.test/");
	await page.waitForFunction(() => window.packageSmoke === true, null, {
		timeout: 90000,
	});
	assert.deepEqual(errors, []);
	assert.equal(await page.locator("canvas").count(), 0);
	console.log(
		"tarball types, bundling, WASM initialization, editor mount and destruction passed",
	);
} finally {
	await browser?.close();
	rmSync(consumer, { recursive: true, force: true });
}
