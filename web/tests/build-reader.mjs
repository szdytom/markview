// Keep the legacy regression host separate from the shipped demo.
import esbuild from "esbuild";
import { cpSync } from "node:fs";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL("../", import.meta.url));
await esbuild.build({
	entryPoints: [`${root}tests/fixtures/reader/main.ts`],
	bundle: true,
	format: "esm",
	outfile: `${root}dist/test-reader.js`,
	target: "es2022",
	alias: { "@markview/web": `${root}packages/web/dist/index.js` },
	loader: { ".otf": "file", ".ttf": "file" },
	assetNames: "assets/[name]-[hash]",
});
cpSync(
	`${root}tests/fixtures/reader/index.html`,
	`${root}dist/test-reader.html`,
);
cpSync(`${root}tests/fixtures/reader/style.css`, `${root}dist/test-reader.css`);
