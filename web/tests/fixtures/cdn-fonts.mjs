import { fileURLToPath } from "node:url";

export const cdnFonts = "https://cdn.jsdelivr.net/gh/**";

// Keep browser regressions independent of CDN availability and download size.
export async function mockCdnFonts(page) {
	await page.route(cdnFonts, (route) => {
		const name = new URL(route.request().url()).pathname
			.split("/")
			.pop()
			.replace("NotoSerifSC-", "NotoSerifCJKsc-")
			.replace("NotoSansSC-", "NotoSansCJKsc-")
			.replace(/\.(otf|ttf)$/, "-subset.otf")
			.replace("NotoColorEmoji-subset.otf", "NotoColorEmoji-subset.ttf");
		return route.fulfill({
			contentType: route.request().url().endsWith(".ttf")
				? "font/ttf"
				: "font/otf",
			path: fileURLToPath(
				new URL(
					`../../../crates/markview-core/tests/fonts/${name}`,
					import.meta.url,
				),
			),
		});
	});
}
