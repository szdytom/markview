// The monorepo's build: the package's `dist/`, then the demo site in `dist/`.
import esbuild from "esbuild";
import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const tsc = join(root, "node_modules/typescript/bin/tsc");

// --- 1. The package: ESM bundle + declarations -------------------------------

const pkg = join(root, "packages/markview");
execFileSync(process.execPath, [join(pkg, "build.mjs")], { stdio: "inherit" });
for (const name of ["scroll-sync", "resources", "fonts", "web", "editor"]) {
	execFileSync(process.execPath, [join(root, "packages/build-package.mjs"), name], { stdio: "inherit" });
}
const alias = Object.fromEntries(["viewer", "scroll-sync", "resources", "fonts", "web", "editor"].map(name =>
	[`@markview/${name}`,join(root,"packages",name === "viewer" ? "markview" : name,"dist/index.js")]));

// --- 2. The demo site: a self-contained static directory ---------------------

const site = join(root, "dist");
rmSync(site, { recursive: true, force: true });
mkdirSync(site, { recursive: true });

// The demo consumes the package's **built** entry point, exactly as a
// downstream application would, so this build also proves the published
// artifact works. The binary travels as a copied sibling, not as a bundler
// asset.
await esbuild.build({
	entryPoints: [join(root, "apps/demo/src/main.ts")],
	bundle: true,
	format: "esm",
	outfile: join(site, "main.js"),
	sourcemap: true,
	target: "es2022",
	alias,
	loader: { ".otf": "file", ".ttf": "file" },
	assetNames: "assets/[name]-[hash]",
});

cpSync(join(root, "apps/demo/index.html"), join(site, "index.html"));
cpSync(join(root, "apps/demo/src/style.css"), join(site, "main.css"));
// `init()` looks for this name beside the module it was loaded from.
cpSync(join(pkg, "dist/markview_web_bg.wasm"), join(site, "markview_web_bg.wasm"));

console.log("built web/dist (index.html, main.js, main.css, markview_web_bg.wasm, assets/)");

// Smoke hosts also consume built public package entries.
for (const [name,entry] of [["api","web"],["editor-api","editor"],["fonts-api","fonts"]]) {
	await esbuild.build({entryPoints:[alias[`@markview/${entry}`]],bundle:true,format:"esm",
		outfile:join(site,`${name}.js`),target:"es2022",alias});
}
await esbuild.build({entryPoints:[join(root,"apps/editor/src/main.ts")],bundle:true,format:"esm",
	outfile:join(site,"editor.js"),sourcemap:true,target:"es2022",alias,
	loader:{".otf":"file",".ttf":"file"},assetNames:"assets/[name]-[hash]"});
cpSync(join(root,"apps/editor/index.html"),join(site,"editor.html"));
await esbuild.build({stdin:{contents:'export * from "@markview/viewer"; export * from "@markview/editor"; export * from "@markview/fonts"; export * from "@markview/resources";',resolveDir:root},bundle:true,format:"esm",outfile:join(site,"integration-api.js"),target:"es2022",alias});
