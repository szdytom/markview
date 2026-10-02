import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";
import { decodePng, inkPixels } from "./png.mjs";

test.setTimeout(120_000);

async function open(page, hash = "read") {
	await page.goto(`/index.html#${hash}`);
	await expect(page.locator("body")).toHaveAttribute("data-ready", "true", {
		timeout: 90_000,
	});
}

const source = (page) => page.getByRole("textbox", { name: "Markdown source" });

async function expectSource(page, text) {
	await expect(source(page).locator(".cm-line")).toHaveText(text.split("\n"));
}

async function replaceSource(page, text) {
	await source(page).fill(text);
	await expect(page.locator("#engine-text")).toHaveText("Ready to read");
}

test("read/edit navigation shares source, canvas, history and document drafts", async ({
	page,
}) => {
	const errors = [];
	page.on("pageerror", (error) => errors.push(error.message));
	await open(page);
	await expect(source(page)).toBeHidden();
	await page
		.context()
		.grantPermissions(["clipboard-read", "clipboard-write"]);
	await page.locator("canvas").press("Control+a");
	await page
		.getByRole("button", { name: "Copy selection", exact: true })
		.click();
	await expect(page.locator("#notice")).toHaveText("Copied selection");
	expect(await page.evaluate(() => navigator.clipboard.readText())).toContain(
		"A note on typography",
	);
	await page.locator("canvas").press("Escape");
	await page.locator("canvas").evaluate((canvas) => {
		canvas.dataset.identity = "original";
	});
	const png = decodePng(await page.locator("canvas").screenshot());
	expect(inkPixels(png, { r: 249, g: 250, b: 252 })).toBeGreaterThan(1000);
	await page.getByRole("link", { name: "Edit", exact: true }).click();
	await replaceSource(page, "# My draft\n\nA shared page.");
	await expect(
		page.getByRole("navigation", { name: "Document outline" }),
	).toContainText("My draft");
	await page.getByRole("link", { name: "Read", exact: true }).click();
	await expect(source(page)).toBeHidden();
	await expect(page.locator("canvas")).toHaveAttribute(
		"data-identity",
		"original",
	);
	await page.goBack();
	await expectSource(page, "# My draft\n\nA shared page.");
	await source(page).press("Control+z");
	await expect(source(page)).toContainText("A note on typography");
	await source(page).press("Control+Shift+z");
	await expect(source(page)).toContainText("My draft");
	await page
		.getByRole("combobox", { name: "Document", exact: true })
		.selectOption("technical");
	await expect(source(page)).toContainText("A technical page");
	await page
		.getByRole("combobox", { name: "Document", exact: true })
		.selectOption("welcome");
	await expectSource(page, "# My draft\n\nA shared page.");
	expect(errors).toEqual([]);
	await expect(page.locator("#error")).toBeHidden();
});

test("file opening and download preserve Unicode and treat filenames as text", async ({
	page,
}) => {
	await open(page, "edit");
	const markdown = "# 中文😀\n\né and **bold**.\n";
	const name = "<img src=x>.md";
	await page.locator("#file").setInputFiles({
		name,
		mimeType: "text/markdown",
		buffer: Buffer.from(markdown),
	});
	await expectSource(page, markdown);
	await expect(page.locator("#document-name")).toHaveText(name);
	await expect(page.locator("#document-name img")).toHaveCount(0);
	await source(page).press("Control+End");
	await source(page).press("Enter");
	await source(page).pressSequentially("Downloaded edit.");
	const promise = page.waitForEvent("download");
	await page.getByRole("button", { name: "Download", exact: true }).click();
	const download = await promise;
	expect(download.suggestedFilename()).toContain(".md");
	const text = await readFile(await download.path(), "utf8");
	expect(text).toBe(markdown + "\nDownloaded edit.");
	await page.locator("#file").setInputFiles({
		name,
		mimeType: "text/markdown",
		buffer: Buffer.from(markdown),
	});
	await expectSource(page, markdown);
});

test("theme and contents controls retain the document across responsive layouts", async ({
	page,
}) => {
	await open(page, "edit");
	await page.getByRole("button", { name: "Dark paper" }).click();
	await expect(page.locator("html")).toHaveAttribute("data-theme", "dark");
	await expect(
		page.getByRole("button", { name: "Light paper" }),
	).toHaveAttribute("aria-pressed", "true");
	await page.getByRole("button", { name: "Contents", exact: true }).click();
	await expect(
		page.getByRole("navigation", { name: "Document outline" }),
	).toBeHidden();
	await page.getByRole("button", { name: "Contents", exact: true }).click();
	await expect(
		page.getByRole("navigation", { name: "Document outline" }),
	).toBeVisible();
	await page.setViewportSize({ width: 390, height: 844 });
	await expect
		.poll(() =>
			page
				.locator(".mv-split")
				.evaluate((el) => getComputedStyle(el).flexDirection),
		)
		.toBe("column");
	expect(
		await page.evaluate(() => document.documentElement.scrollWidth),
	).toBe(390);
	await expect(source(page)).toBeVisible();
	await expect(
		page.getByRole("navigation", { name: "Document outline" }),
	).toBeHidden();
	await page.getByRole("button", { name: "Contents", exact: true }).click();
	await expect(
		page.getByRole("navigation", { name: "Document outline" }),
	).toBeVisible();
	await page
		.getByRole("navigation", { name: "Document outline" })
		.getByRole("button", { name: "The shape of a paragraph", exact: true })
		.click();
	await expect(
		page.getByRole("navigation", { name: "Document outline" }),
	).toBeHidden();
	await page.screenshot({
		path: test.info().outputPath("mobile-edit.png"),
		fullPage: true,
		animations: "disabled",
	});
	await page.getByRole("link", { name: "Read", exact: true }).click();
	await expect(source(page)).toBeHidden();
	await expect(page.locator("canvas")).toBeVisible();
	await page.screenshot({
		path: test.info().outputPath("mobile-read.png"),
		fullPage: true,
		animations: "disabled",
	});
	await page.setViewportSize({ width: 1440, height: 1000 });
	await page.getByRole("button", { name: "Light paper" }).click();
	await page.screenshot({
		path: test.info().outputPath("desktop-read.png"),
		fullPage: true,
		animations: "disabled",
	});
});

test("empty source offers a working edit action and old editor links open the SPA", async ({
	page,
}) => {
	await page.goto("/editor.html");
	await expect(page).toHaveURL(/index\.html#edit$/);
	await expect(page.locator("body")).toHaveAttribute("data-ready", "true", {
		timeout: 90_000,
	});
	await replaceSource(page, "");
	await page.getByRole("link", { name: "Read", exact: true }).click();
	await expect(
		page.getByRole("heading", { name: "Your page starts here." }),
	).toBeVisible();
	await page.getByRole("link", { name: "Write Markdown" }).click();
	await expect(source(page)).toBeVisible();
	await expect(
		page.getByRole("heading", { name: "Your page starts here." }),
	).toBeHidden();
});

test("font startup failure offers recovery and keeps controls disabled", async ({
	page,
}) => {
	await page.route("**/assets/*.otf", (route) => route.abort());
	await page.goto("/index.html");
	await expect(page.getByRole("alert")).toContainText("could not start", {
		timeout: 30_000,
	});
	await expect(page.getByRole("button", { name: "Try again" })).toBeVisible();
	await expect(
		page.getByRole("button", { name: "Open file" }),
	).toBeDisabled();
	await expect(page.locator(".workspace")).toHaveAttribute(
		"aria-busy",
		"false",
	);
	await page.unroute("**/assets/*.otf");
	await page.getByRole("button", { name: "Try again" }).click();
	await expect(page.locator("body")).toHaveAttribute("data-ready", "true", {
		timeout: 90_000,
	});
	await expect(page.getByRole("alert")).toBeHidden();
});

test("the component guide loads from repository Markdown and retains its own draft", async ({
	page,
}) => {
	await open(page);
	await page
		.getByRole("button", { name: "Component guide", exact: true })
		.click();
	await expect(
		page.getByRole("combobox", { name: "Document", exact: true }),
	).toHaveValue("component-guide");
	await expect(page.locator("#document-name")).toHaveText("mvaac.md");
	await expect(
		page.getByRole("navigation", { name: "Document outline" }),
	).toContainText("Packages and ownership");
	const pending = page.waitForEvent("download");
	await page.getByRole("button", { name: "Download", exact: true }).click();
	const download = await pending;
	expect(download.suggestedFilename()).toBe("mvaac.md");
	expect(await readFile(await download.path(), "utf8")).toBe(
		await readFile(new URL("../../docs/mvaac.md", import.meta.url), "utf8"),
	);
	await page.getByRole("link", { name: "Edit", exact: true }).click();
	await replaceSource(page, "# Guide draft\n\nLocal changes.");
	await page
		.getByRole("combobox", { name: "Document", exact: true })
		.selectOption("welcome");
	await page
		.getByRole("combobox", { name: "Document", exact: true })
		.selectOption("component-guide");
	await expectSource(page, "# Guide draft\n\nLocal changes.");
});
