import { readingSetting } from "../shared/compatibility";
import * as vscode from "vscode";
import path from "node:path";
import { randomBytes } from "node:crypto";
import { EditorScrollSync } from "./scroll-sync";
import { needsEmojiFont } from "../shared/preview-fonts";
import { Services } from "../shared/service";
import { resourceDirectory } from "../shared/resources";

export function activate(
  context: vscode.ExtensionContext,
  services = new Services(context),
) {
  const panels = new Map<string, vscode.WebviewPanel>();
  const rendered = new Map<string, { version: number; blocks: number }>();
  const report = (error: unknown) => {
    console.error("Markview preview:", error);
    void vscode.window.showErrorMessage(`Markview：${String(error)}`);
  };
  async function open(
    document: vscode.TextDocument,
    restored?: vscode.WebviewPanel,
    saved?: { base?: string; offset?: number },
  ) {
    const identity = document.uri.toString();
    const existing = panels.get(identity);
    if (existing) {
      existing.reveal(vscode.ViewColumn.Beside);
      return;
    }
    const base =
      saved?.base ?? (await resourceDirectory(document, document.getText()));
    if (!base) return;
    const panel =
      restored ??
      vscode.window.createWebviewPanel(
        "markview.preview",
        "Markview",
        vscode.ViewColumn.Beside,
        { enableScripts: true, retainContextWhenHidden: true },
      );
    panel.title = `预览 · ${path.basename(document.uri.path)}`;
    panels.set(identity, panel);
    services.previewDocument = document.uri;
    const media = vscode.Uri.joinPath(context.extensionUri, "media");
    const uri = (name: string) =>
      panel.webview.asWebviewUri(vscode.Uri.joinPath(media, name)).toString();
    let disposed = false,
      generation = 0;
    let fontPaths:
      Promise<{ fonts: string[]; stylesheet?: string }> | undefined;
    let emojiFonts = false;
    const resources = new Set<AbortController>();
    const subscriptions: vscode.Disposable[] = [];
    const scrollSync = new EditorScrollSync(
      panel,
      document,
      subscriptions,
      saved?.offset !== undefined,
    );
    let edit: ReturnType<EditorScrollSync["edit"]>;
    async function publish() {
      const ticket = ++generation;
      const markdown = document.getText(),
        version = document.version;
      const config = vscode.workspace.getConfiguration("markview", document);
      const template = await services.readingTemplate(document.uri);
      if (disposed || ticket !== generation) return;
      const emoji = needsEmojiFont(markdown);
      if (emoji !== emojiFonts) {
        fontPaths = undefined;
        emojiFonts = emoji;
      }
      fontPaths ??= (async () => {
        const [catalog, stylesheet] = await Promise.all([
          services.catalog(template, "ui"),
          services.previewStylesheet(template),
        ]);
        if (!emoji)
          catalog.definitions = catalog.definitions.filter(
            ({ id }) => id !== "emoji",
          );
        await services.fonts.refresh(services.directories());
        return { fonts: services.fonts.paths(catalog), stylesheet };
      })().catch((error) => {
        fontPaths = undefined;
        throw error;
      });
      const { fonts, stylesheet } = await fontPaths;
      if (disposed || ticket !== generation) return;
      panel.webview.options = {
        enableScripts: true,
        localResourceRoots: [
          media,
          vscode.Uri.file(base!),
          ...fonts.map((file) => vscode.Uri.file(path.dirname(file))),
        ],
      };
      await panel.webview.postMessage({
        type: "document",
        uri: identity,
        version,
        markdown,
        maxLine: scrollSync.maxLine(),
        edit: edit?.version === version ? edit : undefined,
        template: template.id,
        stylesheet,
        codeWrap: readingSetting(config, "codeWrap", "codeblockWrap", false),
        options: {
          fontSize: config.get("fontSize", 18),
          width: readingSetting(config, "width", "columnWidth", 760),
          justify: config.get("justify", true),
          hyphenate: config.get("hyphenate", true),
          paragraphIndent: config.get("paragraphIndent", 0),
        },
        base,
        offset: saved?.offset,
        fonts: fonts.map((file) =>
          panel.webview.asWebviewUri(vscode.Uri.file(file)).toString(),
        ),
        theme: [
          vscode.ColorThemeKind.Dark,
          vscode.ColorThemeKind.HighContrast,
        ].includes(vscode.window.activeColorTheme.kind)
          ? "dark"
          : "light",
      });
      saved = undefined;
    }
    const update = () => void publish().catch(report);
    subscriptions.push(
      panel.onDidChangeViewState(({ webviewPanel }) => {
        if (webviewPanel.active) services.previewDocument = document.uri;
      }),
      panel.webview.onDidReceiveMessage(async (message) => {
        try {
          if (message.type === "ready") {
            update();
            return;
          }
          if (message.type === "error") {
            report(message.error);
            return;
          }
          if (message.uri !== identity || message.version !== document.version)
            return;
          if (message.type === "rendered") {
            rendered.set(identity, {
              version: message.version,
              blocks: message.blocks,
            });
            if (message.version === edit?.version) edit = undefined;
          }
          if (scrollSync.receive(message)) return;
          if (message.type === "link" && typeof message.target === "string") {
            if (message.target.startsWith("#")) return;
            const target = vscode.Uri.parse(
              new URL(
                message.target,
                vscode.Uri.file(base + path.sep).toString(),
              ).href,
            );
            if (target.scheme === "file")
              await vscode.commands.executeCommand("vscode.open", target);
            else if (["https", "http", "mailto"].includes(target.scheme))
              await vscode.env.openExternal(target);
          } else if (
            message.type === "resource" &&
            typeof message.target === "string"
          ) {
            const controller = new AbortController();
            resources.add(controller);
            try {
              const target = new URL(
                message.target,
                vscode.Uri.file(base + path.sep).toString(),
              );
              let bytes: Uint8Array;
              if (target.protocol === "file:")
                bytes = await vscode.workspace.fs.readFile(
                  vscode.Uri.parse(target.href),
                );
              else if (["http:", "https:"].includes(target.protocol)) {
                const response = await fetch(target, {
                  signal: controller.signal,
                });
                if (!response.ok)
                  throw new Error(`Image request failed: ${response.status}`);
                bytes = new Uint8Array(await response.arrayBuffer());
              } else throw new Error("Unsupported image URL");
              if (!disposed && !controller.signal.aborted)
                await panel.webview.postMessage({
                  ...message,
                  bytes: Buffer.from(bytes).toString("base64"),
                });
            } catch (error) {
              if (!disposed && !controller.signal.aborted)
                await panel.webview.postMessage({
                  ...message,
                  error: String(error),
                });
            } finally {
              resources.delete(controller);
            }
          }
        } catch (error) {
          report(error);
        }
      }),
      vscode.workspace.onDidChangeTextDocument((event) => {
        if (event.document !== document) return;
        let delta = 0;
        const ends = [...event.contentChanges]
          .sort((a, b) => a.rangeOffset - b.rangeOffset)
          .map((change) => {
            const end = change.rangeOffset + delta + change.text.length;
            delta += change.text.length - change.rangeLength;
            return end;
          });
        const editor = vscode.window.activeTextEditor;
        const caret =
          editor?.document === document && ends.length
            ? document.offsetAt(editor.selection.active)
            : 0;
        const offset = ends.reduce(
          (best, end) =>
            Math.abs(end - caret) < Math.abs(best - caret) ? end : best,
          ends[0] ?? caret,
        );
        edit = ends.length ? scrollSync.edit(offset) : undefined;
        resources.forEach((controller) => controller.abort());
        update();
      }),
      vscode.window.onDidChangeActiveColorTheme(update),
      vscode.workspace.onDidChangeConfiguration((event) => {
        if (
          event.affectsConfiguration("markview") ||
          event.affectsConfiguration("markviewExport")
        ) {
          if (
            [
              "markview.template",
              "markviewExport.template",
              "markview.previewTemplate",
              "markview.previewStyleFile",
              "markview.fontDirectories",
            ].some((key) => event.affectsConfiguration(key))
          ) {
            fontPaths = undefined;
          }
          update();
        }
      }),
      services.changed.event(() => {
        fontPaths = undefined;
        update();
      }),
      panel.onDidDispose(() => {
        disposed = true;
        generation++;
        panels.delete(identity);
        rendered.delete(identity);
        if (services.previewDocument === document.uri)
          services.previewDocument = undefined;
        resources.forEach((controller) => controller.abort());
        subscriptions.forEach((subscription) => subscription.dispose());
      }),
    );
    panel.webview.options = {
      enableScripts: true,
      localResourceRoots: [media],
    };
    const nonce = randomBytes(18).toString("base64");
    panel.webview.html = `<!doctype html><html><head><meta charset="UTF-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; connect-src ${panel.webview.cspSource} data:; img-src blob: data:; script-src 'nonce-${nonce}' 'wasm-unsafe-eval'; style-src ${panel.webview.cspSource} 'unsafe-inline';"><link rel="stylesheet" href="${uri("preview.css")}"></head><body><span id="status" role="status">正在加载…</span><main id="reader"></main><script nonce="${nonce}">window.markviewHost=acquireVsCodeApi();</script><script nonce="${nonce}" type="module" src="${uri("preview.js")}" data-wasm="${uri("markview_web_bg.wasm")}"></script></body></html>`;
  }
  context.subscriptions.push(
    vscode.commands.registerCommand("markview.preview.template", async () => {
      try {
        const template = await services.pickTemplate("ui");
        if (!template) return;
        const config = vscode.workspace.getConfiguration("markview");
        const target = vscode.workspace.workspaceFolders?.length
          ? vscode.ConfigurationTarget.Workspace
          : vscode.ConfigurationTarget.Global;
        await config.update(
          "previewTemplate",
          template.path ? "default" : template.id,
          target,
        );
        await config.update("previewStyleFile", template.path ?? "", target);
      } catch (error) {
        report(error);
      }
    }),
    vscode.commands.registerCommand(
      "markview.preview.open",
      async (uri?: vscode.Uri) => {
        const document = uri
          ? await vscode.workspace.openTextDocument(uri)
          : vscode.window.activeTextEditor?.document;
        if (document?.languageId === "markdown")
          await open(document).catch(report);
      },
    ),
    vscode.window.registerWebviewPanelSerializer("markview.preview", {
      async deserializeWebviewPanel(panel, raw) {
        const saved = raw as
          { uri?: unknown; base?: unknown; offset?: unknown } | undefined;
        try {
          if (typeof saved?.uri !== "string")
            throw new Error("No saved document URI");
          const document = await vscode.workspace.openTextDocument(
            vscode.Uri.parse(saved.uri),
          );
          await open(document, panel, {
            base: typeof saved.base === "string" ? saved.base : undefined,
            offset: typeof saved.offset === "number" ? saved.offset : 0,
          });
        } catch (error) {
          panel.webview.html =
            "<!doctype html><p>无法恢复预览，请重新打开源文件。</p>";
          report(error);
        }
      },
    }),
    {
      dispose: () => {
        for (const panel of panels.values()) panel.dispose();
      },
    },
  );
  return { status: (uri: string) => rendered.get(uri) };
}
