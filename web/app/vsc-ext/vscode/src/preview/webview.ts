import {
  Viewer,
  type FontSet,
  type ImageRequest,
  type MarkviewOptions,
} from "@markview/viewer";
import { FontLoader } from "@markview/fonts";
import { browserResources, decodeImage } from "@markview/resources";
import { PreviewScrollSync, type EditPosition } from "./scroll-sync-webview";
import { PreviewScrollbar } from "./scrollbar";
declare global {
  interface Window {
    markviewHost: {
      postMessage(message: unknown): void;
      setState(state: unknown): void;
    };
  }
}
interface DocumentMessage {
  type: "document";
  uri: string;
  version: number;
  markdown: string;
  maxLine?: number;
  edit?: EditPosition;
  options?: MarkviewOptions;
  template?: string;
  stylesheet?: string;
  codeWrap?: boolean;
  fonts: string[];
  theme: "light" | "dark";
  base: string;
  offset?: number;
}
const host = window.markviewHost;
const status = document.querySelector<HTMLElement>("#status")!;
const wasmUrl =
  document.querySelector<HTMLScriptElement>("script[data-wasm]")!.dataset.wasm!;
const loader = new FontLoader();
const resources = new Map<number, ImageRequest>();
let viewer: Viewer | undefined,
  fonts: FontSet | undefined,
  scrollbar: PreviewScrollbar | undefined;
let current: DocumentMessage | undefined,
  fontKey = "",
  generation = 0,
  resourceId = 0,
  disposed = false;
let updates = Promise.resolve();
let renderedVersion = -1;
const send = (type: string, extra = {}) =>
  host.postMessage({
    type,
    uri: current?.uri,
    version: current?.version,
    ...extra,
  });
const sync = new PreviewScrollSync(send);
const fail = (error: unknown) => {
  status.textContent = String(error);
  send("error", { error: String(error) });
};
async function update(message: DocumentMessage, ticket: number) {
  const key = JSON.stringify(message.fonts);
  if (key !== fontKey || !fonts) {
    const next = await loader.load({ sources: message.fonts }, { wasmUrl });
    if (ticket !== generation || disposed) {
      next.destroy();
      return;
    }
    viewer?.setFonts(next);
    fonts?.destroy();
    fonts = next;
    fontKey = key;
  }
  if (ticket !== generation || disposed) return;
  const previous = current;
  current = message;
  sync.document(
    message.version,
    message.markdown,
    message.maxLine ?? message.markdown.split("\n").length - 1,
    message.edit,
  );
  if (!viewer) {
    viewer = await Viewer.mount(
      document.querySelector<HTMLElement>("#reader")!,
      {
        markdown: message.markdown,
        fonts,
        initialization: { wasmUrl },
        scrollMode: "external",
        markview: { ...message.options, theme: message.theme },
        onLink: (target) => send("link", { target }),
        onError: fail,
        onUserInput: () => sync.control(),
        onStats: (stats) => {
          sync.frame();
          scrollbar?.update();
          if (
            !stats.pending &&
            current &&
            renderedVersion !== current.version
          ) {
            renderedVersion = current.version;
            send("rendered", { blocks: stats.blocks });
          }
        },
        onReadingPosition: (position) => {
          sync.reading(position);
          host.setState({
            uri: current?.uri,
            base: current?.base,
            offset: position.offset,
          });
        },
        resources: {
          onResources: (events) => {
            for (const event of events)
              if (event.kind === "request") {
                if (event.request.src.startsWith("data:")) {
                  browserResources({ onError: fail }).onResources?.([event]);
                  continue;
                }
                const id = ++resourceId;
                resources.set(id, event.request);
                event.request.signal.addEventListener(
                  "abort",
                  () => resources.delete(id),
                  { once: true },
                );
                send("resource", { id, target: event.request.src });
              }
          },
          onError: fail,
        },
      },
    );
    if (disposed) {
      viewer.destroy();
      viewer = undefined;
      return;
    }
    sync.attach(viewer);
    scrollbar = new PreviewScrollbar(viewer, (y) => {
      sync.control();
      viewer?.scrollTo(y);
    });
    if (message.offset !== undefined) viewer.scrollToSource(message.offset);
  } else {
    if (viewer.getMarkdown() !== message.markdown)
      viewer.setMarkdown(message.markdown);
    if (
      previous?.theme !== message.theme ||
      JSON.stringify(previous?.options) !== JSON.stringify(message.options)
    )
      viewer.setOptions({ ...message.options, theme: message.theme });
  }
  if (
    !previous ||
    previous.template !== message.template ||
    previous.stylesheet !== message.stylesheet ||
    previous.codeWrap !== message.codeWrap ||
    previous.theme !== message.theme
  ) {
    viewer.registerStylesheet(
      "vscode-config",
      `format_version=3\nversion=1\n[[rule]]\nwhen=["code_block"]\nwrap=${message.codeWrap ?? false}`,
    );
    if (message.stylesheet)
      viewer.registerStylesheet("vscode-custom", message.stylesheet);
    const base =
      message.template && !["default", "custom"].includes(message.template)
        ? `bundled:${message.template}`
        : `bundled:${message.theme}`;
    viewer.setStylesheets([
      "vscode-config",
      ...(message.stylesheet ? ["vscode-custom"] : []),
      base,
    ]);
  }
  host.setState({
    uri: message.uri,
    base: message.base,
    offset: viewer.readingPosition()?.offset ?? message.offset ?? 0,
  });
  status.textContent = message.fonts.length
    ? ""
    : "未找到阅读字体，请配置 Markview 字体目录。";
}
window.addEventListener("message", (event) => {
  const message = event.data;
  if (message.type === "document") {
    const ticket = ++generation;
    updates = updates
      .then(() =>
        ticket === generation && !disposed
          ? update(message, ticket)
          : undefined,
      )
      .catch(fail);
  } else if (message.type === "source-scroll") {
    if (message.uri === current?.uri) sync.source(message);
  } else if (message.type === "resource") {
    const request = resources.get(message.id);
    resources.delete(message.id);
    if (
      !request ||
      request.signal.aborted ||
      message.uri !== current?.uri ||
      message.version !== current?.version
    )
      return;
    if (message.error) request.reject(message.error);
    else
      void decodeImage(
        Uint8Array.from(atob(message.bytes), (c) => c.charCodeAt(0)),
        request.signal,
      ).then(
        (pixels) => request.resolve(pixels),
        (error) => request.reject(String(error)),
      );
  }
});
window.addEventListener("pagehide", () => {
  disposed = true;
  generation++;
  resources.clear();
  scrollbar?.destroy();
  viewer?.destroy();
  fonts?.destroy();
  loader.clear();
});
host.postMessage({ type: "ready" });
