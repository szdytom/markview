import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
import { runInNewContext } from "node:vm";

const bundle = await build({
  entryPoints: ["vscode/src/preview/scrollbar.ts"],
  bundle: true,
  platform: "node",
  format: "cjs",
  write: false,
});

function preview(content = 2000, height = 600) {
  const elements = [];
  const document = {
    createElement() {
      const element = {
        style: {},
        attributes: {},
        classList: { add() {}, remove() {} },
        append() {},
        focus() {},
        setPointerCapture() {},
        addEventListener() {},
        setAttribute(key, value) {
          this.attributes[key] = value;
        },
        getBoundingClientRect() {
          return {
            top: 40 + (parseFloat(this.style.transform?.slice(11)) || 0),
          };
        },
      };
      elements.push(element);
      return element;
    },
  };
  const module = { exports: {} };
  runInNewContext(bundle.outputFiles[0].text, {
    document,
    module,
    exports: module.exports,
  });
  const state = { content, height, y: 0 };
  const engine = {
    maxScroll: () => Math.max(0, state.content - Math.min(20, state.height)),
    contentHeight: () => state.content,
    scroll: () => state.y,
  };
  const scrollbar = new module.exports.PreviewScrollbar(
    {
      reader: { markview: engine },
      element: { clientHeight: height, parentElement: { append() {} } },
    },
    (y) => {
      state.y = Math.max(0, Math.min(engine.maxScroll(), y));
      scrollbar.update();
    },
  );
  scrollbar.update();
  return { scrollbar, state, track: elements[0], thumb: elements[1] };
}

test("the thumb represents visible content and shrinks in the blank tail", () => {
  const { scrollbar, state, track, thumb } = preview();
  for (const [y, start, height] of [
    [0, 0, 180],
    [1400, 420, 180],
    [1600, 480, 120],
    [1800, 540, 60],
    [1980, 594, 6],
  ]) {
    state.y = y;
    scrollbar.update();
    assert.ok(Math.abs(parseFloat(thumb.style.height) - height) < 0.0001);
    assert.equal(thumb.style.transform, `translateY(${start}px)`);
    assert.equal(track.attributes["aria-valuenow"], String(y));
    assert.equal(track.attributes["aria-valuemax"], "1980");
  }
});

test("short documents start fully visible; long documents keep exact proportions", () => {
  const short = preview(400);
  assert.equal(short.thumb.style.height, "600px");
  short.state.y = 200;
  short.scrollbar.update();
  assert.equal(short.thumb.style.height, "300px");
  assert.equal(short.thumb.style.transform, "translateY(300px)");
  assert.equal(preview(0).track.hidden, true);
  assert.equal(preview(20).track.hidden, true);
  assert.equal(preview(1_000_000).thumb.style.height, "0.36px");
});

test("dragging stays stable while the thumb shrinks and can reach both limits", () => {
  const { state, track, thumb } = preview();
  const pointer = (clientY) => ({
    clientY,
    button: 0,
    pointerId: 1,
    target: thumb,
    preventDefault() {},
  });
  track.onpointerdown(pointer(50));
  assert.equal(state.y, 0);
  for (const y of [1400, 1600, 1980, 1600, 0]) {
    track.onpointermove(pointer(50 + (y / 2000) * 600));
    assert.equal(state.y, y);
    track.onpointermove(pointer(50 + (y / 2000) * 600));
    assert.equal(state.y, y);
  }
  track.onpointermove(pointer(10000));
  assert.equal(state.y, 1980);
  track.onpointermove(pointer(-10000));
  assert.equal(state.y, 0);
  track.onlostpointercapture();
  track.onpointermove(pointer(10000));
  assert.equal(state.y, 0);
});

test("End and Home reach the content limits and update accessibility values", () => {
  const { state, track, thumb } = preview();
  let prevented = 0;
  const key = (key) => ({
    key,
    preventDefault() {
      prevented++;
    },
  });
  track.onkeydown(key("End"));
  assert.equal(state.y, 1980);
  assert.equal(track.attributes["aria-valuenow"], "1980");
  assert.ok(Math.abs(parseFloat(thumb.style.height) - 6) < 0.0001);
  track.onkeydown(key("ArrowDown"));
  assert.equal(state.y, 1980);
  track.onkeydown(key("Home"));
  assert.equal(state.y, 0);
  assert.equal(track.attributes["aria-valuenow"], "0");
  track.onkeydown(key("ArrowUp"));
  assert.equal(state.y, 0);
  track.onkeydown(key("Escape"));
  assert.equal(prevented, 4);
});
