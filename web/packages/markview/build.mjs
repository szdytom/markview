// Builds the package's `dist/`: one ESM bundle plus `.d.ts` from `tsc`.
import esbuild from "esbuild";
import { execFileSync } from "node:child_process";
import { cpSync, mkdirSync, rmSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = dirname(fileURLToPath(import.meta.url));
const dist = join(root, "dist");

rmSync(dist, { recursive: true, force: true });
mkdirSync(dist, { recursive: true });

// The wasm glue is bundled in; the binary is copied beside it under the plain
// name `init()` looks for, so no bundler has to know this package ships one.
await esbuild.build({
	entryPoints: [join(root, "src/index.ts")],
	bundle: true,
	format: "esm",
	outfile: join(dist, "index.js"),
	sourcemap: true,
	target: "es2022",
});

// `--outDir` is resolved against the process cwd, which `pnpm --dir web` sets
// to the monorepo root, so it has to be absolute to land in this package.
execFileSync(
	process.execPath,
	[join(root, "../../node_modules/typescript/bin/tsc"), "--emitDeclarationOnly", "--declaration", "--noEmit", "false", "--outDir", dist, "-p", root],
	{ stdio: "inherit", cwd: root },
);

// The glue's `.d.ts` files describe the wasm module's surface, and the binary
// itself sits beside the JavaScript: `init()` resolves it by name, so it must
// keep that name rather than a content hash.
for (const file of ["markview_web.d.ts", "markview_web_bg.wasm.d.ts", "markview_web_bg.wasm"]) {
	cpSync(join(root, "wasm", file), join(dist, file));
}

console.log("built packages/markview/dist");

for (const file of readdirSync(dist).filter(file => file.endsWith(".d.ts"))) {
	const path = join(dist,file);
	writeFileSync(path,readFileSync(path,"utf8").replaceAll("../wasm/","./"));
}
