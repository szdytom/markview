import { downloadAndUnzipVSCode } from "@vscode/test-electron";
import { spawn, execFileSync } from "node:child_process";
import {
  mkdtemp,
  mkdir,
  writeFile,
  rm,
  readdir,
  readFile,
} from "node:fs/promises";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const vsix = path.resolve(process.env.MARKVIEW_VSIX);
const editor =
  process.env.VSCODE_EXEC_PATH || (await downloadAndUnzipVSCode("stable"));
const scratch = await mkdtemp(path.join(os.tmpdir(), "mv-install-"));
const cli =
  process.platform === "darwin"
    ? path.resolve(path.dirname(editor), "../Resources/app/bin/code")
    : path.join(
        path.dirname(editor),
        "bin",
        process.platform === "win32" ? "code.cmd" : "code",
      );
async function run(command, args, timeout = 180000) {
  const child = spawn(command, args, {
    stdio: "inherit",
    env: { ...process.env },
    detached: process.platform !== "win32",
  });
  const timer = setTimeout(() => {
    if (process.platform !== "win32") process.kill(-child.pid, "SIGKILL");
    else child.kill();
  }, timeout);
  try {
    await new Promise((resolve, reject) => {
      child.on("error", reject);
      child.on("exit", (code) =>
        code === 0
          ? resolve()
          : reject(new Error(`Test process exited ${code}`)),
      );
    });
  } finally {
    clearTimeout(timer);
  }
}
try {
  const workspace = path.join(scratch, "workspace");
  await mkdir(workspace, { recursive: true });
  const user = path.join(scratch, "user"),
    extensions = path.join(scratch, "extensions");
  await mkdir(path.join(user, "User"), { recursive: true });
  await writeFile(
    path.join(user, "User/settings.json"),
    JSON.stringify({
      "markviewExport.template": "mondrian",
      "update.mode": "none",
      "extensions.autoUpdate": false,
    }),
  );
  const common = ["--user-data-dir", user, "--extensions-dir", extensions];
  if (process.env.MARKVIEW_OLD_VSIX)
    await run(cli, [
      ...common,
      "--install-extension",
      path.resolve(process.env.MARKVIEW_OLD_VSIX),
      "--force",
    ]);
  await run(cli, [...common, "--install-extension", vsix, "--force"]);
  const names = await readdir(extensions);
  const manifest = JSON.parse(
    await readFile(path.join(root, "vscode/package.json"), "utf8"),
  );
  const current = names.filter((name) =>
    name.startsWith(`stevvven.markview-export-${manifest.version}`),
  );
  if (current.length !== 1)
    throw new Error(`New package not installed: ${names}`);
  await run(editor, [
    workspace,
    ...common,
    "--extensionDevelopmentPath=" + path.join(extensions, current[0]),
    "--extensionTestsPath=" + path.join(root, "tests/installed.cjs"),
    "--disable-workspace-trust",
    "--no-sandbox",
    "--skip-welcome",
    "--skip-release-notes",
    "--disable-gpu",
    "--enable-unsafe-swiftshader",
  ]);
} finally {
  if (process.platform !== "win32") {
    const owned = execFileSync("ps", ["-axo", "pid=,command="], {
      encoding: "utf8",
    })
      .split("\n")
      .filter(
        (line) => line.includes(scratch) && line.includes("/bin/markview"),
      );
    for (const line of owned)
      process.kill(Number(line.trim().split(/\s+/)[0]), "SIGKILL");
    await rm(scratch, { recursive: true, force: true });
    if (owned.length) throw new Error("An exporter outlived the test window");
    console.log("MARKVIEW-INTEGRATION no exporter outlived the window");
  } else await rm(scratch, { recursive: true, force: true });
}
