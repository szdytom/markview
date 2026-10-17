import { promises as fs } from "node:fs";
import os from "node:os";
import path from "node:path";
import { installFamily } from "./font-store";
import { runNative } from "./process";
import type { Catalog, Template, Progress } from "./types";

// Share the native font protocol across editor hosts.
export class FontClient {
  constructor(
    readonly binary: string,
    readonly cache: string,
  ) {}
  private args(template: Template | undefined, destination: string): string[] {
    return [
      "--destination",
      destination,
      ...(template?.path
        ? ["--file", template.path]
        : template && template.id !== "default"
          ? ["--style", template.id]
          : []),
    ];
  }
  async catalog(template?: Template, destination = "pdf"): Promise<Catalog> {
    return JSON.parse(
      await runNative(this.binary, [
        "fonts",
        "catalog",
        ...this.args(template, destination),
      ]),
    );
  }
  async withTemplate<T>(
    template: Template,
    operation: (snapshot: Template) => Promise<T>,
  ): Promise<T> {
    if (!template.path) return operation(template);
    const source = await fs.readFile(template.path);
    const staging = await fs.mkdtemp(
      path.join(os.tmpdir(), "markview-template-"),
    );
    const file = path.join(staging, "template.mvss.toml");
    try {
      await fs.writeFile(file, source);
      return await operation({ ...template, path: file });
    } finally {
      await fs.rm(staging, { recursive: true, force: true });
    }
  }
  async downloadFamily(
    template: Template,
    destination: string,
    id: string,
    signal?: AbortSignal,
    progress?: (event: Progress) => void,
  ) {
    const catalog = await this.catalog(template, destination);
    const family = catalog.families.find((family) => family.id === id);
    if (!family) throw new Error(`Unknown font family: ${id}`);
    await installFamily(
      this.cache,
      family,
      async (directory) => {
        await runNative(
          this.binary,
          [
            "fonts",
            "download",
            "--json",
            id,
            "--directory",
            directory,
            ...this.args(template, destination),
          ],
          "",
          signal,
          progress,
        );
      },
      signal,
    );
  }
}
