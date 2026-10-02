// Globals the demo page exposes for the Playwright harness.
declare global {
	interface Window {
		/** Set once the engine has presented its first frame. */
		__markviewReady: boolean;
		/** Set when boot fails; the banner shows the same text. */
		__markviewError?: string;
		/** The component the page drives; the tests use the same surface. */
		mv: import("@markview/web").Markview;
		/** The parsed stats read-back. */
		mvStats(): import("@markview/web").MarkviewStats;
		/** The reader that owns the frame loop. */
		mvReader: import("@markview/web").CanvasReader;
	}

	/** Configuration injected by the page or a test before the module runs. */
	var MV_CONFIG: import("@markview/web").MarkviewOptions | string | undefined;
}

export {};
