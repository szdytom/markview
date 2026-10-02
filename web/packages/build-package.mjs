import esbuild from "esbuild";
import { execFileSync } from "node:child_process";
import {
	cpSync,
	existsSync,
	mkdirSync,
	readdirSync,
	readFileSync,
	rmSync,
	writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
const root = dirname(fileURLToPath(import.meta.url));
const pkg = join(root, process.argv[2]);
const dist = join(pkg, "dist");
const viewer = process.argv[2] === "markview";
if (viewer && !existsSync(join(pkg, "wasm/markview_web.js"))) {
	console.error(
		"WASM bindings are missing. Run pnpm --dir web run setup, then pnpm --dir web build.",
	);
	process.exit(1);
}
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
	{ stdio: "inherit", cwd: pkg },
);

if (viewer) {
	// `init()` resolves the binary beside the bundled module.
	for (const file of [
		"markview_web.d.ts",
		"markview_web_bg.wasm.d.ts",
		"markview_web_bg.wasm",
	]) {
		cpSync(join(pkg, "wasm", file), join(dist, file));
	}
	for (const file of readdirSync(dist).filter((file) =>
		file.endsWith(".d.ts"),
	)) {
		const path = join(dist, file);
		writeFileSync(
			path,
			readFileSync(path, "utf8").replaceAll("../wasm/", "./"),
		);
	}
}
console.log(`Built packages/${process.argv[2]}/dist`);
