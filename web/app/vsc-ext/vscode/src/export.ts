import * as vscode from "vscode";
import path from "node:path";
import { resourceDirectory } from "./shared/resources";
import { Services } from "./shared/service";
import { exportSnapshot } from "./shared/export-snapshot";
import type { Template } from "./shared/types";

export function activate(
  context: vscode.ExtensionContext,
  services = new Services(context),
) {
  async function exportDocument(
    format?: "pdf" | "png",
    withTemplate = false,
    uri?: vscode.Uri,
    destination?: vscode.Uri,
  ) {
    const document = uri
      ? await vscode.workspace.openTextDocument(uri)
      : vscode.window.activeTextEditor?.document;
    if (!document || document.languageId !== "markdown") {
      await vscode.window.showWarningMessage("请先打开 Markdown 文档");
      return;
    }
    const text = document.getText();
    const settings = vscode.workspace.getConfiguration("markview", document);
    const template: Template | undefined = withTemplate
      ? await services.pickTemplate()
      : await services.template("pdf", document.uri);
    if (!template) return;
    return services.withTemplate(template, async (template) => {
      const chosen =
        format ??
        (await vscode.window.showQuickPick(["PDF", "PNG"], {
          title: "选择导出格式",
        }));
      if (!chosen) return;
      const extension = chosen.toLowerCase();
      const base = await resourceDirectory(document, text);
      if (!base) return;
      const stem = document.isUntitled
        ? "Untitled"
        : path.basename(document.uri.fsPath, path.extname(document.uri.fsPath));
      const output =
        destination ??
        (await vscode.window.showSaveDialog({
          defaultUri: vscode.Uri.file(path.join(base, `${stem}.${extension}`)),
          filters: { [extension.toUpperCase()]: [extension] },
        }));
      if (!output) return;
      if (output.scheme !== "file") throw new Error("首版只支持保存到本地文件");
      await vscode.window.withProgress(
        {
          location: vscode.ProgressLocation.Notification,
          title: `导出 ${extension.toUpperCase()}`,
          cancellable: true,
        },
        async (progress, token) => {
          const controller = new AbortController();
          const subscription = token.onCancellationRequested(() =>
            controller.abort(),
          );
          let percent = 0;
          try {
            await exportSnapshot(
              services,
              text,
              base,
              extension as "pdf" | "png",
              output.fsPath,
              template,
              controller.signal,
              (event) => {
                const next = Math.max(percent, (event.fraction ?? 0) * 100);
                progress.report({
                  message: event.message ?? event.phase,
                  increment: next - percent,
                });
                percent = next;
              },
              {
                fontSize: settings.get("exportFontSize", 16),
                scale: settings.get("exportScale", 2),
                trusted: vscode.workspace.isTrusted,
              },
            );
          } finally {
            subscription.dispose();
          }
        },
      );
      if (destination) return;
      void vscode.window.showInformationMessage(`已导出 ${output.fsPath}`);
      try {
        await vscode.commands.executeCommand("revealFileInOS", output);
      } catch (error) {
        await vscode.window.showWarningMessage(
          `文件已导出，但无法在文件管理器中显示：${String(error)}`,
        );
      }
    });
  }
  const command = (
    name: string,
    action: (uri?: vscode.Uri) => Promise<unknown>,
  ) =>
    context.subscriptions.push(
      vscode.commands.registerCommand(name, async (uri?: vscode.Uri) => {
        try {
          await action(uri);
        } catch (error) {
          if (!(error instanceof Error && error.name === "AbortError")) {
            await vscode.window.showErrorMessage(String(error));
          }
        }
      }),
    );
  command("markview.export.pdf", (uri) => exportDocument("pdf", false, uri));
  command("markview.export.png", (uri) => exportDocument("png", false, uri));
  command("markview.export.template", (uri) =>
    exportDocument(undefined, true, uri),
  );
  return exportDocument;
}
