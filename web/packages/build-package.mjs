import esbuild from "esbuild";
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const root = dirname(fileURLToPath(import.meta.url));
const pkg = join(root, process.argv[2]);
const dist = join(pkg, "dist");
rmSync(dist, { recursive: true, force: true });
mkdirSync(dist, { recursive: true });
await esbuild.build({
	entryPoints: [join(pkg, "src/index.ts")],
	bundle: true,
	format: "esm",
	outfile: join(dist, "index.js"),
	target: "es2022",
	sourcemap: true,
	packages: "external",
});
execFileSync(
	process.execPath,
	[
		join(root, "../node_modules/typescript/bin/tsc"),
		"-p",
		pkg,
		"--noEmit",
		"false",
		"--declaration",
		"--emitDeclarationOnly",
		"--outDir",
		dist,
	],
	{ stdio: "inherit" },
);
