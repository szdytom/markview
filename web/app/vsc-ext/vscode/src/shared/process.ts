import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";
import type { Progress } from "./types";

export async function runNative(
  binary: string,
  args: string[],
  input = "",
  signal?: AbortSignal,
  progress?: (event: Progress) => void,
): Promise<string> {
  const staging =
    args[0] === "export" || (args[0] === "fonts" && args[1] === "download")
      ? await fs.mkdtemp(path.join(os.tmpdir(), "markview-cancel-"))
      : undefined;
  const marker = staging ? path.join(staging, "cancel") : undefined;
  try {
    return await new Promise<string>((resolve, reject) => {
      if (signal?.aborted) {
        reject(new DOMException("Cancelled", "AbortError"));
        return;
      }
      const child = spawn(
        binary,
        marker ? [...args, "--cancel-file", marker] : args,
        {
          stdio: ["pipe", "pipe", "pipe"],
          windowsHide: true,
        },
      );
      let stdout = "",
        stderr = "",
        timer: NodeJS.Timeout | undefined;
      const abort = () => {
        if (marker) void fs.writeFile(marker, "").catch(() => child.kill());
        else child.kill();
        timer = setTimeout(() => child.kill("SIGKILL"), marker ? 5000 : 1500);
        timer.unref();
      };
      signal?.addEventListener("abort", abort, { once: true });
      const lines = createInterface({ input: child.stdout });
      lines.on("line", (line) => {
        stdout = (stdout + line + "\n").slice(-2_000_000);
        try {
          progress?.(JSON.parse(line) as Progress);
        } catch {
          /* Non-protocol logs are retained for diagnostics. */
        }
      });
      child.stderr.on("data", (chunk) => {
        stderr = (stderr + chunk).slice(-32_000);
      });
      child.stdin.on("error", () => {});
      child.stdin.end(input);
      child.on("error", finish);
      child.on("close", (code) =>
        finish(
          signal?.aborted
            ? new DOMException("Cancelled", "AbortError")
            : code === 0
              ? undefined
              : new Error(stderr.trim() || `Markview exited with code ${code}`),
        ),
      );
      function finish(error?: Error) {
        signal?.removeEventListener("abort", abort);
        if (timer) clearTimeout(timer);
        lines.close();
        if (error) reject(error);
        else resolve(stdout);
      }
    });
  } finally {
    if (staging) await fs.rm(staging, { recursive: true, force: true });
  }
}
