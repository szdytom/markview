// Find Python on each platform; pass paths as arguments without a shell.
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const candidates =
	process.platform === "win32"
		? [["py", "-3"], ["python"], ["python3"]]
		: [["python3"], ["python"]];
const python = candidates.find(
	([command, ...args]) =>
		spawnSync(command, [
			...args,
			"-c",
			"import sys; sys.exit(sys.version_info < (3, 11))",
		]).status === 0,
);
if (!python) {
	console.error(
		"Install Python 3.11 or newer, add it to PATH, then retry this command.",
	);
	process.exit(1);
}
const [command, ...args] = python;
const result = spawnSync(
	command,
	[
		...args,
		fileURLToPath(new URL("../../scripts/build-web.py", import.meta.url)),
		...process.argv.slice(2),
	],
	{ stdio: "inherit" },
);
if (result.error) console.error(result.error.message);
process.exit(result.status ?? 1);
