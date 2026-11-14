import type { Viewer } from "@markview/viewer";

/** A persistent, keyboard-accessible scrollbar for the canvas viewport. */
export class PreviewScrollbar {
  private track = document.createElement("div");
  private thumb = document.createElement("div");
  private dragOffset: number | undefined;
  private height = 0;
  private thumbHeight = 0;
  private contentHeight = 0;
  private max = 0;
  private last = "";

  constructor(
    private viewer: Viewer,
    navigate: (y: number) => void,
  ) {
    const track = this.track;
    track.className = "preview-scrollbar";
    track.tabIndex = 0;
    track.setAttribute("role", "scrollbar");
    track.setAttribute("aria-label", "预览滚动条");
    track.setAttribute("aria-orientation", "vertical");
    track.setAttribute("aria-controls", "reader");
    track.setAttribute("aria-valuemin", "0");
    this.thumb.className = "preview-scrollbar-thumb";
    track.append(this.thumb);
    viewer.element.parentElement!.append(track);
    const move = (event: PointerEvent) => {
      const y =
        event.clientY - track.getBoundingClientRect().top - this.dragOffset!;
      navigate(
        Math.max(
          0,
          Math.min(
            this.max,
            (y * this.contentHeight) / Math.max(1, this.height),
          ),
        ),
      );
    };
    track.onpointerdown = (event) => {
      if (event.button !== 0) return;
      event.preventDefault();
      track.focus();
      track.setPointerCapture(event.pointerId);
      track.classList.add("dragging");
      this.dragOffset =
        event.target === this.thumb
          ? event.clientY - this.thumb.getBoundingClientRect().top
          : this.thumbHeight / 2;
      move(event);
    };
    track.onpointermove = (event) => {
      if (this.dragOffset !== undefined) move(event);
    };
    track.onlostpointercapture = () => {
      this.dragOffset = undefined;
      track.classList.remove("dragging");
    };
    track.onkeydown = (event) => {
      const y = viewer.reader.markview.scroll();
      const target = {
        ArrowUp: y - 42,
        ArrowDown: y + 42,
        PageUp: y - this.height * 0.9,
        PageDown: y + this.height * 0.9,
        Home: 0,
        End: this.max,
      }[event.key];
      if (target !== undefined) {
        event.preventDefault();
        navigate(Math.max(0, Math.min(this.max, target)));
      }
    };
    track.addEventListener(
      "wheel",
      (event) => {
        event.preventDefault();
        const scale =
          event.deltaMode === 1 ? 16 : event.deltaMode === 2 ? this.height : 1;
        navigate(viewer.reader.markview.scroll() + event.deltaY * scale);
      },
      { passive: false },
    );
  }

  update() {
    const engine = this.viewer.reader.markview;
    this.height = this.viewer.element.clientHeight;
    this.max = engine.maxScroll();
    this.contentHeight = engine.contentHeight();
    const y = engine.scroll();
    const key = `${this.height}:${this.max}:${this.contentHeight}:${y}`;
    if (key === this.last) return;
    this.last = key;
    this.track.hidden = this.max <= 0 || this.contentHeight <= 0;
    const start = Math.min(1, y / Math.max(1, this.contentHeight));
    const end = Math.min(
      1,
      (y + this.height) / Math.max(1, this.contentHeight),
    );
    this.thumbHeight = (end - start) * this.height;
    this.thumb.style.height = `${this.thumbHeight}px`;
    this.thumb.style.transform = `translateY(${start * this.height}px)`;
    this.track.setAttribute("aria-valuemax", String(Math.round(this.max)));
    this.track.setAttribute("aria-valuenow", String(Math.round(y)));
  }

  destroy() {
    this.track.remove();
  }
}
