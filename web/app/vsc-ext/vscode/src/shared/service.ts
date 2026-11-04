import * as vscode from "vscode";
import path from "node:path";
import { readFile } from "node:fs/promises";
import { FontClient } from "./font-client";
import { FontIndex } from "./fonts";
import { explicit, legacyTemplate } from "./compatibility";
import { TemplateSettings } from "./template-settings";
import type { Template } from "./types";

export class Services extends FontClient implements vscode.Disposable {
  readonly changed = new vscode.EventEmitter<void>();
  private templates = new TemplateSettings(() => this.changed.fire());
  readonly fonts: FontIndex;
  previewDocument?: vscode.Uri;
  constructor(readonly context: vscode.ExtensionContext) {
    super(
      context.asAbsolutePath(
        path.join(
          "bin",
          process.platform === "win32" ? "markview.exe" : "markview",
        ),
      ),
      path.join(context.globalStorageUri.fsPath, "fonts"),
    );
    this.fonts = new FontIndex(
      this.cache,
      path.join(context.globalStorageUri.fsPath, "font-index.json"),
    );
  }

  directories(): string[] {
    return vscode.workspace
      .getConfiguration("markview")
      .get<string[]>("fontDirectories", []);
  }
  async template(destination: string, uri?: vscode.Uri): Promise<Template> {
    const inherited = await this.templates.resolve(uri, destination);
    const scope = uri ? { uri, languageId: "markdown" } : undefined;
    const config = vscode.workspace.getConfiguration("markview", scope);
    const prefix = destination === "ui" ? "preview" : "export";
    const file = config.get<string>(`${prefix}StyleFile`, "");
    const id = config.get<string>(`${prefix}Template`, "default");
    const modern =
      inherited ||
      explicit(config, `${prefix}StyleFile`) ||
      explicit(config, `${prefix}Template`);
    if (!modern) {
      const old = explicit(config, "template")
        ? config.get<string>("template")
        : destination !== "ui"
          ? vscode.workspace
              .getConfiguration("markviewExport", scope)
              .get<string>("template")
          : undefined;
      if (old !== undefined) {
        const template = legacyTemplate(old, uri);
        if (template.path) this.templates.watch(template.path);
        return template;
      }
    }
    const template =
      inherited ??
      (file
        ? { id: "custom", name: path.basename(file), path: file }
        : { id, name: id });
    if (template.path) this.templates.watch(template.path);
    return template;
  }
  readingTemplate(uri?: vscode.Uri): Promise<Template> {
    return this.template("ui", uri);
  }
  async previewStylesheet(template: Template): Promise<string | undefined> {
    const file = template.path;
    return file ? readFile(file, "utf8") : undefined;
  }
  async pickTemplate(destination = "pdf"): Promise<Template | undefined> {
    const { templates } = await this.catalog(undefined, destination);
    const ui = destination === "ui";
    const bundled = [
      "light",
      "dark",
      "celadon",
      "blueprint",
      "rosewood",
      "8-bit",
    ];
    const choice = await vscode.window.showQuickPick(
      [
        {
          label: ui ? "跟随 VS Code 主题" : "默认导出样式",
          template: { id: "default", name: "Default" } as Template,
        },
        ...templates
          .filter((template) => !ui || bundled.includes(template.id))
          .map((template) => ({
            label: template.name,
            description: template.id,
            template,
          })),
        { label: "自定义 .mvss.toml…", template: undefined },
      ],
      { title: ui ? "选择阅读模板" : "选择导出模板" },
    );
    if (!choice) return;
    if (choice.template) return choice.template;
    const files = await vscode.window.showOpenDialog({
      canSelectMany: false,
      filters: { MVSS: ["mvss.toml", "toml"] },
      title: "选择 MVSS 模板",
    });
    return files?.[0]
      ? {
          id: "custom",
          name: path.basename(files[0].fsPath),
          path: files[0].fsPath,
        }
      : undefined;
  }
  override async downloadFamily(
    ...args: Parameters<FontClient["downloadFamily"]>
  ) {
    await super.downloadFamily(...args);
    this.changed.fire();
  }
  dispose() {
    this.templates.dispose();
    this.changed.dispose();
  }
}
