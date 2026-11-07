import * as vscode from "vscode";
import { FontPanel } from "./shared/font-panel";
import { activate as activatePreview } from "./preview/extension";
import { activate as activateExport } from "./export";
import { Services } from "./shared/service";

export function activate(context: vscode.ExtensionContext) {
  const services = new Services(context);
  const preview = activatePreview(context, services);
  const exportDocument = activateExport(context, services);
  const aliases = {
    "markviewExport.exportPdf": "markview.export.pdf",
    "markviewExport.exportPng": "markview.export.png",
    "markviewExport.exportWithTemplate": "markview.export.template",
    "markview.exportPdf": "markview.export.pdf",
    "markview.exportPng": "markview.export.png",
    "markview.exportWithTemplate": "markview.export.template",
    "markview.openPreview": "markview.preview.open",
  };
  for (const [previous, current] of Object.entries(aliases))
    context.subscriptions.push(
      vscode.commands.registerCommand(previous, (...args: unknown[]) =>
        vscode.commands.executeCommand(current, ...args),
      ),
    );
  let panel: FontPanel | undefined;
  context.subscriptions.push(
    services,
    { dispose: () => panel?.dispose() },
    vscode.commands.registerCommand("markview.fonts", async () => {
      try {
        const document =
          vscode.window.activeTextEditor?.document.uri ??
          services.previewDocument;
        if (!panel || panel.disposed) panel = new FontPanel(services);
        await panel.show(undefined, "ui", document);
      } catch (error) {
        void vscode.window.showErrorMessage(`Markview: ${String(error)}`);
      }
    }),
  );
  return {
    previewStatus: preview.status,
    configuredTemplate: (destination: string, uri: vscode.Uri) =>
      services.template(destination, uri),
    exportDocument,
  };
}
