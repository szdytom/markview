const cdn = "https://cdn.jsdelivr.net/gh/";
const sans = `${cdn}notofonts/noto-cjk@Sans2.004/Sans/`;
const fontsource = "https://cdn.jsdelivr.net/npm/@fontsource/";

function webFonts(family: string, subset: string, styles: readonly string[]) {
	// The SC v4 faces retain names recognized by the bundled stylesheet.
	const version = family.endsWith("-sc") ? "4.5.12" : "5.2.9";
	return styles.map(
		(style) =>
			new URL(
				`${family}@${version}/files/${family}-${subset}-${style}.woff2`,
				fontsource,
			),
	);
}

// Explicit web subsets, independent of the regression fixtures.
export const fonts = [
	...webFonts("noto-serif", "latin", [
		"400-normal",
		"700-normal",
		"400-italic",
	]),
	...webFonts("noto-sans", "latin", [
		"400-normal",
		"700-normal",
		"400-italic",
	]),
	...webFonts("noto-sans-mono", "latin", ["400-normal", "700-normal"]),
	...webFonts("noto-serif-sc", "chinese-simplified", [
		"400-normal",
		"700-normal",
	]),
	...webFonts("noto-sans-sc", "chinese-simplified", [
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
