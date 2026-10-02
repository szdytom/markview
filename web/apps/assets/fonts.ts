const cdn = "https://cdn.jsdelivr.net/gh/";
const latin = `${cdn}notofonts/noto-fonts@ffebf8c1ee449e544955a7e813c54f9b73848eac/hinted/ttf/`;
const serif = `${cdn}notofonts/noto-cjk@Serif2.003/Serif/SubsetOTF/SC/`;
const sans = `${cdn}notofonts/noto-cjk@Sans2.004/Sans/`;

// Load upstream faces with their full language coverage, independent of test fixtures.
export const fonts = [
	...[
		"NotoSerif/NotoSerif-Regular.ttf",
		"NotoSerif/NotoSerif-Bold.ttf",
		"NotoSerif/NotoSerif-Italic.ttf",
		"NotoSans/NotoSans-Regular.ttf",
		"NotoSans/NotoSans-Bold.ttf",
		"NotoSans/NotoSans-Italic.ttf",
		"NotoSansMono/NotoSansMono-Regular.ttf",
		"NotoSansMono/NotoSansMono-Bold.ttf",
	].map((file) => new URL(file, latin)),
	...["NotoSerifSC-Regular.otf", "NotoSerifSC-Bold.otf"].map(
		(file) => new URL(file, serif),
	),
	...[
		"SubsetOTF/SC/NotoSansSC-Regular.otf",
		"SubsetOTF/SC/NotoSansSC-Medium.otf",
		"SubsetOTF/SC/NotoSansSC-Bold.otf",
		"Mono/NotoSansMonoCJKsc-Regular.otf",
		"Mono/NotoSansMonoCJKsc-Bold.otf",
	].map((file) => new URL(file, sans)),
	new URL(`${cdn}googlefonts/noto-emoji@v2.051/fonts/NotoColorEmoji.ttf`),
];
