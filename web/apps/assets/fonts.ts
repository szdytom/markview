const cdn = "https://cdn.jsdelivr.net/gh/";
const sans = `${cdn}notofonts/noto-cjk@Sans2.004/Sans/`;
export const fontFallbacks = new Map<string, URL>();

function webFonts(
	family: string,
	version: string,
	subset: string,
	styles: readonly string[],
	variable = false,
) {
	const scope = variable ? "@fontsource-variable" : "@fontsource";
	return styles.map((style) => {
		const file = `files/${family}-${subset}-${style}.woff2`;
		const url = new URL(
			`https://registry.npmmirror.com/${scope}/${family}/${version}/files/${file}`,
		);
		fontFallbacks.set(
			url.href,
			new URL(
				`https://cdn.jsdelivr.net/npm/${scope}/${family}@${version}/${file}`,
			),
		);
		return url;
	});
}

// Explicit web subsets, independent of the regression fixtures.
export const fonts = [
	// One variable face covers every weight from 100 to 900.
	...webFonts(
		"noto-serif",
		"5.3.0",
		"latin",
		["wght-normal", "wght-italic"],
		true,
	),
	...webFonts(
		"noto-sans",
		"5.3.0",
		"latin",
		["wght-normal", "wght-italic"],
		true,
	),
	...webFonts("noto-sans-mono", "5.3.0", "latin", ["wght-normal"], true),
	// The SC v4 faces retain names recognized by the bundled stylesheet.
	...webFonts("noto-serif-sc", "4.5.12", "chinese-simplified", [
		"400-normal",
		"500-normal",
		"600-normal",
		"700-normal",
	]),
	...webFonts("noto-sans-sc", "4.5.12", "chinese-simplified", [
		"400-normal",
		"500-normal",
		"700-normal",
	]),
	// Preserve full CJK monospace coverage and bitmap color emoji.
	...["NotoSansMonoCJKsc-Regular.otf", "NotoSansMonoCJKsc-Bold.otf"].map(
		(file) => new URL(`Mono/${file}`, sans),
	),
	new URL(`${cdn}googlefonts/noto-emoji@v2.051/fonts/NotoColorEmoji.ttf`),
];
