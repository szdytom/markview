import { fileURLToPath } from "node:url";
import { readFile } from "node:fs/promises";
import { deflateSync } from "node:zlib";

export const cdnFonts = "https://cdn.jsdelivr.net/**";

// Lossless WOFF fixtures exercise the WASM decoder without new binary assets.
function woff(sfnt) {
	const count = sfnt.readUInt16BE(4);
	let offset = 44 + count * 20;
	const directory = Buffer.alloc(offset);
	directory.write("wOFF");
	sfnt.copy(directory, 4, 0, 4);
	directory.writeUInt16BE(count, 12);
	directory.writeUInt32BE(sfnt.length, 16);
	const tables = [];
	for (let i = 0; i < count; i++) {
		const record = 12 + i * 16;
		const original = sfnt.subarray(
			sfnt.readUInt32BE(record + 8),
			sfnt.readUInt32BE(record + 8) + sfnt.readUInt32BE(record + 12),
		);
		const compressed = deflateSync(original);
		const data =
			compressed.length < original.length ? compressed : original;
		const entry = 44 + i * 20;
		sfnt.copy(directory, entry, record, record + 4);
		directory.writeUInt32BE(offset, entry + 4);
		directory.writeUInt32BE(data.length, entry + 8);
		directory.writeUInt32BE(original.length, entry + 12);
		sfnt.copy(directory, entry + 16, record + 4, record + 8);
		const padded = Buffer.alloc((data.length + 3) & ~3);
		data.copy(padded);
		tables.push(padded);
		offset += padded.length;
	}
	directory.writeUInt32BE(offset, 8);
	return Buffer.concat([directory, ...tables]);
}

// Keep browser regressions independent of CDN availability and download size.
export async function mockCdnFonts(page) {
	await page.route(cdnFonts, async (route) => {
		const url = new URL(route.request().url());
		let name = url.pathname
			.split("/")
			.pop()
			.replace("NotoSerifSC-", "NotoSerifCJKsc-")
			.replace("NotoSansSC-", "NotoSansCJKsc-")
			.replace(/\.(otf|ttf)$/, "-subset.otf")
			.replace("NotoColorEmoji-subset.otf", "NotoColorEmoji-subset.ttf");
		if (url.pathname.endsWith(".woff2")) {
			const family = url.pathname.split("/")[3].split("@")[0];
			const families = {
				"noto-serif": "NotoSerif",
				"noto-sans": "NotoSans",
				"noto-sans-mono": "NotoSansMono",
				"noto-serif-sc": "NotoSerifCJKsc",
				"noto-sans-sc": "NotoSansCJKsc",
			};
			const [, weight, style] = url.pathname.match(
				/-(\d+)-(normal|italic)\.woff2$/,
			);
			const face =
				style === "italic"
					? "Italic"
					: weight === "700"
						? "Bold"
						: weight === "500"
							? "Medium"
							: "Regular";
			name = `${families[family]}-${face}-subset.otf`;
			if (family === "noto-serif" && face === "Regular")
				return route.fulfill({
					contentType: "font/woff2",
					path: fileURLToPath(
						new URL(
							"../../../crates/markview-web/tests/fonts/NotoSerif-Regular-subset.woff2",
							import.meta.url,
						),
					),
				});
			return route.fulfill({
				contentType: "font/woff",
				body: woff(
					await readFile(
						new URL(
							`../../../crates/markview-core/tests/fonts/${name}`,
							import.meta.url,
						),
					),
				),
			});
		}
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
