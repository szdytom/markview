import * as vscode from "vscode";
import path from "node:path";
import type { Template } from "./types";

export function explicit(
  config: vscode.WorkspaceConfiguration,
  key: string,
): boolean {
  const value = config.inspect?.(key);
  return value
    ? [
        value.globalValue,
        value.workspaceValue,
        value.workspaceFolderValue,
        value.globalLanguageValue,
        value.workspaceLanguageValue,
        value.workspaceFolderLanguageValue,
      ].some((entry) => entry !== undefined)
    : config.get(key) !== undefined;
}

export function readingSetting<T>(
  config: vscode.WorkspaceConfiguration,
  key: string,
  previous: string,
  fallback: T,
): T {
  return !explicit(config, key) && explicit(config, previous)
    ? config.get<T>(previous, fallback)
    : config.get<T>(key, fallback);
}

export function legacyTemplate(value: string, uri?: vscode.Uri): Template {
  const name = value.trim();
  if (!name) return { id: "default", name: "Default" };
  if (name.endsWith(".mvss.toml") || /[/\\]/.test(name)) {
    const base =
      uri?.scheme === "file"
        ? path.dirname(uri.fsPath)
        : (uri && vscode.workspace.getWorkspaceFolder(uri)?.uri.fsPath) ||
          vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (!path.isAbsolute(name) && !base)
      throw new Error("Save the document or use an absolute template path.");
    return {
      id: "custom",
      name: path.basename(name),
      path: path.resolve(base ?? "", name),
    };
  }
  return { id: name, name };
}
