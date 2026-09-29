// Runs inside a real VS Code extension host, so `vscode` is the editor's own.
const assert = require("node:assert");
const { execSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

/** Engine processes alive right now. */
function engines() {
	try {
		const found = execSync("ps -axo pid=,command=", {
			encoding: "utf8",
		});
		return found.split("\n").filter((line) => line.includes((process.env.MARKVIEW_TEST_BINARY ?? path.resolve(__dirname, "../../../target/release/markview")) + " serve ")).map((line) => line.trim().split(/\s+/)[0]);
	} catch {
		return [];
	}
}

function say(label, value) {
	console.log(`MARKVIEW-EXT ${label} ${JSON.stringify(value)}`);
}

/** Browser scrolling rounds to physical pixels, before conversion to layout units. */
function scrollPixel(report) {
    const tile = report.tileBounds[0];
    assert.ok(tile?.scale > 0 && tile?.fit > 0, "painted display scale is known");
    return 1 / (tile.scale * tile.fit) + 0.01;
}

function delay(ms) {
	return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Scrolls the preview and waits for the effect.
 *
 * The webview is a separate process: a scroll posted while it is still coming
 * up is dropped, so the scroll is sent again if nothing has happened.
 */
async function scrollPreview(exports, offset, changed, attempts = 6) {
	for (let attempt = 0; attempt < attempts; attempt += 1) {
		await exports.scrollPreviewTo(offset);
		for (let wait = 0; wait < 12; wait += 1) {
			await delay(100);
			if (changed(exports.panelReport())) {
				return true;
			}
		}
	}
	return false;
}

/**
 * Whether the bands fetched since `from` cover the rectangle a scroll to
 * `top` shows. A band that a tile was fetched for elsewhere in the document
 * says nothing about this rectangle, so only the overlapping ones count.
 */
function covers(offsets, from, top, view) {
	const bands = [...new Set(offsets.slice(from))]
		.filter((offset) => offset < top + view && offset + view > top)
		.sort((a, b) => a - b);
	let reach = top;
	for (const offset of bands) {
		if (offset > reach) {
			return false;
		}
		reach = Math.max(reach, offset + view);
	}
	return reach >= top + view;
}

exports.run = async function () {
	const vscode = require("vscode");

	/** Whether a tab is this extension's preview. */
	const isPreview = (tab) =>
		tab?.input instanceof vscode.TabInputWebview &&
		// The editor prefixes a webview tab's view type, so the panel is
		// recognised by the name this extension gave it.
		tab.input.viewType.endsWith("markview.preview");
	/** The group the preview sits in, if it is open at all. */
	const previewGroup = () =>
		vscode.window.tabGroups.all.find((group) => group.tabs.some(isPreview));
	/** Whether the preview is the tab on top of its group. */
	const previewIsVisible = () =>
		vscode.window.tabGroups.all.some((group) => isPreview(group.activeTab));

	// The extension ships no binary of its own yet, so the test points it at
	// the one the engine's own build produced.
	const repository = path.resolve(__dirname, "../../..");
	const binary = process.env.MARKVIEW_TEST_BINARY ?? path.join(repository, "target/release/markview");
	assert.ok(fs.existsSync(binary), `the engine is built at ${binary}`);
	await vscode.workspace
		.getConfiguration("markview")
		.update("enginePath", process.env.MARKVIEW_PACKAGED_EXTENSION ? "" : binary, vscode.ConfigurationTarget.Global);

	const extension = vscode.extensions.getExtension(
		"Stevvven.markview-export",
	);
	assert.ok(extension, "the extension is installed");
	await extension.activate();
	if (process.env.MARKVIEW_REGRESSIONS_ONLY) {
		const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "markview-ui-"));
		try { await require("./regressions.js").run(extension, scratch); say("ok", true); }
		finally { fs.rmSync(scratch, { recursive: true, force: true }); }
		return;
	}
	if (process.env.MARKVIEW_LATENCY_ONLY) {
		const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "markview-latency-"));
		try { await require("./latency.js").run(extension.exports, scratch); say("ok", true); }
		finally { fs.rmSync(scratch, { recursive: true, force: true }); }
		return;
	}
	const registered = await vscode.commands.getCommands(true);
	for (const id of [
		"markview.openPreview",
		"markview.closePreview",
		"markview.exportPdf",
		"markview.exportPng",
		"markview.exportWithTemplate",
		"markview.revealSource",
	]) {
		assert.ok(registered.includes(id), `${id} is registered`);
	}
	assert.strictEqual(engines().length, 0, "nothing runs before the first use");

	// Two documents opened in turn must share one engine: starting it is the
	// expensive part of a preview, so a window pays for it once.
	const scratch = fs.mkdtempSync(path.join(os.tmpdir(), "markview-ext-"));
	const first = path.join(scratch, "first.md");
	const second = path.join(scratch, "second.md");
	fs.writeFileSync(first, "# First\n\nOne paragraph.\n");
	fs.writeFileSync(second, "# Second\n\nAnother paragraph.\n");

	for (const file of [first, second]) {
		const document = await vscode.workspace.openTextDocument(file);
		await vscode.window.showTextDocument(document);
		await vscode.commands.executeCommand("markview.openPreview");
	}
	const secondUri = vscode.Uri.file(second).toString();
	// The engine starts asynchronously; give it a moment to appear.
	let alive = [];
	for (let attempt = 0; attempt < 40; attempt += 1) {
		alive = engines();
		if (alive.length > 0) {
			break;
		}
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	assert.strictEqual(alive.length, 1, `one engine, not ${alive.length}`);
	say("engines-after-two-documents", alive.length);
	say("engine-pids", alive);

	// EXT-2: the preview is a panel in the editor's own grid, so it can sit
	// beside the source the way any other editor can.
	const opened = previewGroup();
	assert.ok(opened, "the preview is open as a panel");
	say("preview-tabs", opened ? 1 : 0);

	// It occupies a group of its own, beside the source rather than over it:
	// the source is put back in the first group and both stay visible.
	const secondDocument = await vscode.workspace.openTextDocument(second);
	await vscode.window.showTextDocument(secondDocument, vscode.ViewColumn.One);
	await delay(300);
	const previewColumn = opened.viewColumn;
	const sourceColumns = vscode.window.tabGroups.all
		.flatMap((group) => group.tabs)
		.filter(
			(tab) =>
				tab.input instanceof vscode.TabInputText &&
				tab.input.uri.toString() === secondUri,
		)
		.map((tab) => tab.group.viewColumn);
	assert.ok(
		sourceColumns.length > 0 && sourceColumns[0] !== previewColumn,
		`the preview is beside the source: preview ${previewColumn}, source ${sourceColumns}`,
	);
	// Two groups are on screen: the source in one, the preview in the other.
	const groups = vscode.window.tabGroups.all.map((group) => ({
		column: group.viewColumn,
		tabs: group.tabs.map((tab) =>
			tab.input instanceof vscode.TabInputWebview ? "preview" : "source",
		),
	}));
	assert.ok(
		groups.length >= 2 &&
			groups.some((group) => group.tabs.includes("preview")) &&
			groups.some((group) => group.tabs.includes("source")),
		`the two surfaces sit side by side: ${JSON.stringify(groups)}`,
	);
	say("preview-column", previewColumn);
	say("source-column", sourceColumns[0]);
	say("groups", groups);

	// The two surfaces the editor builds its entry points from are declared
	// in the manifest, which is what puts the command in the palette and the
	// editor title; the command itself ran above.
	const menus = extension.packageJSON.contributes.menus;
	const title = (menus["editor/title"] ?? []).find(
		(entry) => entry.command === "markview.openPreview",
	);
	assert.ok(
		title && /editorLangId\s*==\s*markdown/.test(String(title.when)),
		"the editor title offers the preview on Markdown",
	);
	assert.ok(
		(menus.commandPalette ?? []).some(
			(entry) => entry.command === "markview.openPreview",
		),
		"the command palette lists the preview",
	);
	say("menus", {
		title: title.when,
		palette: (menus.commandPalette ?? []).length,
	});

	// Asking for the preview shows it again, even when another tab in its
	// group is covering it: reloading the panel out of sight would leave the
	// reader with a document they cannot see.
	await vscode.window.showTextDocument(secondDocument, {
		viewColumn: previewColumn,
		preserveFocus: true,
	});
	await delay(400);
	assert.strictEqual(
		previewIsVisible(),
		false,
		"the source covers the preview to begin with",
	);
	await vscode.commands.executeCommand(
		"markview.openPreview",
		secondDocument.uri,
	);
	await delay(400);
	assert.strictEqual(
		previewIsVisible(),
		true,
		"asking for the preview brings it back to the front",
	);
	say("preview-visible-after-reopen", previewIsVisible());

	// The window's engine is ended when the window ends, which the runner
	// checks after this host exits; here the panel is closed and the engine is
	// expected to stay for the next document.
	await vscode.commands.executeCommand("markview.closePreview");
	await new Promise((resolve) => setTimeout(resolve, 200));
	assert.strictEqual(
		engines().length,
		1,
		"the engine is kept for the next document",
	);
	say("engines-after-closing-the-panel", engines().length);

	// EXT-3: a long document's scrollable extent is its own height, and only
	// the bands in view are fetched rather than the whole document.
	const long = path.join(scratch, "long.md");
	// The text is deliberately not ASCII: the engine counts source offsets in
	// UTF-8 bytes and the editor counts them in UTF-16 code units, so every
	// sync below is a conversion as well as a scroll.
	const longText = Array.from(
		{ length: 400 },
		(_, index) => `第${index}段：這是一段用來測試非 ASCII 位移的文字，長度足夠換行。\n\n`,
	).join("");
	fs.writeFileSync(long, longText);
	const exports = extension.exports;
	const before = exports.panelReport();
	const longDocument = await vscode.workspace.openTextDocument(long);
	// The reader is looking at the source when the preview is opened, so the
	// source gets the first group; the editor title hands the command the
	// resource it was clicked on, which is the route taken here.
	await vscode.window.showTextDocument(longDocument, {
		viewColumn: vscode.ViewColumn.One,
		preserveFocus: false,
	});
	await vscode.commands.executeCommand(
		"markview.openPreview",
		longDocument.uri,
	);

	// The same document asked of an engine of this host's own, with the same
	// settings and the same appearance the extension set. The panel's extent
	// is only right if it is the geometry the engine itself reports.
	const { Session } = require(path.join(extension.extensionPath, "out/shared/sidecar.js"));
	const settings = {
		font_size: 18,
		width: 760,
		justify: false,
		hyphenate: true,
		paragraph_indent: 0,
		codeblock_wrap: false,
	};
	const probe = new Session(binary, [
		"--state-dir",
		path.join(scratch, "probe-state"),
	]);
	await probe.appearance({ theme: "light" });
	const probed = await probe.open("probe", longDocument.getText(), {
		path: long,
		settings,
	});

    const binaryTile = await probe.tile("probe", { width: 80, height: 40 });
    assert.ok(binaryTile.png instanceof Uint8Array, "engine pixels arrive as binary bytes");
    assert.deepEqual([...binaryTile.png.subarray(0, 8)], [137, 80, 78, 71, 13, 10, 26, 10]);

	let observed = before;
	for (let attempt = 0; attempt < 100; attempt += 1) {
		observed = exports.panelReport();
		if (
			observed.ready &&
			observed.documentHeight > 2000 &&
			Math.abs(observed.scrollHeight - observed.documentHeight) <= 2 &&
			observed.live >= 1
		) {
			break;
		}
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	assert.ok(observed.ready, "the webview reported itself ready");
	assert.ok(
		observed.documentHeight > 2000,
		`the document is long: ${observed.documentHeight}`,
	);
	assert.strictEqual(
		Math.abs(observed.openedHeight - probed.height) <= 1,
		true,
		`the panel laid the document out at ${observed.openedHeight}, the engine at ${probed.height}`,
	);
	assert.strictEqual(
		observed.blocks,
		probed.blocks.length,
		"the panel's document has as many blocks as the engine's",
	);
	assert.ok(
		Math.abs(observed.scrollHeight - observed.documentHeight) <= 2,
		`the scroll extent ${observed.scrollHeight} is the document height ${observed.documentHeight}`,
	);
	assert.ok(
		observed.viewport > 0,
		`the webview reported its viewport: ${observed.viewport}`,
	);
	// The document is several screens long, so virtualization is a claim with
	// something to be wrong about.
	const screens = Math.ceil(observed.documentHeight / observed.viewport);
	assert.ok(screens >= 5, `the document is ${screens} screens long`);

	// A fresh panel has asked for the band in view and nothing else.
	const fresh = observed.requested.length - before.requested.length;
	assert.ok(fresh >= 1, "the visible band was fetched");
	assert.ok(fresh <= 2, `a fresh panel asked for ${fresh} bands, not one`);
	assert.ok(
		observed.live >= 1 && observed.live <= 3,
		`the webview holds ${observed.live} bands at once: ${JSON.stringify(observed)}`,
	);

	// Scrolling the preview fetches the bands the viewport comes to show, and
	// they cover it: a tile is a whole screenful or a crop of one, and a
	// scroll rarely lands on a band boundary. Repeating a scroll inside the
	// band already on screen fetches nothing, so the number of bands held
	// stays bounded however far the reader goes.
	const seen = [];
	for (const fraction of [0.5, 0.5, 0.9, 0.2]) {
		const top = Math.max(0, Math.floor(Math.min(observed.documentHeight * fraction, observed.documentHeight - observed.viewport)));
		await scrollPreview(
			exports,
			top,
			(report) =>
				report.live > 0 &&
				covers(
					report.requested,
					before.requested.length,
					top,
					report.viewport,
				),
		);
		const at = exports.panelReport();
		seen.push({
			fraction,
			top,
			live: at.live,
			viewport: at.viewport,
			fetched: at.requested.length - before.requested.length,
			covers: covers(at.requested, before.requested.length, top, at.viewport),
			offsets: at.requested.slice(before.requested.length),
		});
	}
	const after = exports.panelReport();
	const fetched = after.requested.length - before.requested.length;
	assert.ok(
		Math.abs(after.documentHeight - observed.documentHeight) <= 2 &&
			Math.abs(after.scrollHeight - observed.documentHeight) <= 2,
		"the extent is the document height throughout",
	);
	assert.ok(
		after.live >= 1 && after.live <= 3,
		`the webview still holds only ${after.live} bands`,
	);
	// Every scroll position is covered by the bands fetched for it, the one
	// the panel opened at included, and every band was asked for once.
	for (const step of seen) {
		assert.ok(
			step.covers,
			`the bands fetched at ${step.fraction} of the document cover the viewport: ${JSON.stringify(step)}`,
		);
	}
	assert.ok(
		covers(observed.requested, 0, 0, observed.viewport),
		"the opening band covers the viewport",
	);
	assert.strictEqual(
		seen[1].fetched,
		seen[0].fetched,
		"a second scroll within the same band fetched nothing",
	);
	const offsets = after.requested.slice(
		before.requested.length + seen[0].fetched,
	);
	assert.strictEqual(
		new Set(offsets).size,
		offsets.length,
		`no band was fetched twice once the viewport settled: ${JSON.stringify(offsets)}`,
	);
	assert.ok(fetched >= 3, `scrolling fetched the bands it moved to: ${fetched}`);
	assert.ok(
		fetched <= fresh + 6,
		`three distinct scrolls fetch at most two bands each: ${fetched} including ${fresh} initial bands`,
	);
	for (const step of seen) {
		say(`scroll-${step.fraction}`, {
			live: step.live,
			fetched: step.fetched,
			covers: step.covers,
		});
	}
	const cumulative = seen.map((step) => step.fetched);
	assert.ok(
		cumulative[0] > 0 &&
			cumulative[1] === cumulative[0] &&
			cumulative[2] > cumulative[1] &&
			cumulative[3] > cumulative[2],
		`the fetches follow the scrolls: ${JSON.stringify(cumulative)}`,
	);
	// A document the panel has moved on from is not the document it shows.
	// Editing one that was previewed earlier must leave this preview alone:
	// its extent, and the tiles it is holding, are the ones for the document
	// on screen.
	const left = await vscode.workspace.openTextDocument(first);
	const settled = exports.panelReport();
    const backgroundEdit = new vscode.WorkspaceEdit();
    backgroundEdit.insert(left.uri, new vscode.Position(0, 0), "Edited after leaving.\n\n");
    assert.ok(await vscode.workspace.applyEdit(backgroundEdit));
	await delay(1500);
	const untouched = exports.panelReport();
	assert.ok(
		Math.abs(untouched.documentHeight - settled.documentHeight) <= 2 &&
			Math.abs(untouched.scrollHeight - settled.scrollHeight) <= 2,
		`editing a document the panel left behind kept its geometry: ${settled.documentHeight} then ${untouched.documentHeight}, extent ${untouched.scrollHeight}`,
	);
	assert.ok(
		untouched.live >= 1,
		`the preview kept its tiles: ${untouched.live}`,
	);
	say("left-behind-edit", {
		height: Math.round(untouched.documentHeight),
		extent: Math.round(untouched.scrollHeight),
		live: untouched.live,
		bands: untouched.requested.length,
	});

	// EXT-4: a burst of keystrokes reaches the preview coalesced, and what it
	// converges on is the last state of the buffer rather than one of the
	// intermediate ones. The edits are applied one at a time, because two
	// edits of the same document cannot be in flight at once: what is a burst
	// is that they are issued back to back, not that they overlap.
	const editor = await vscode.window.showTextDocument(longDocument, {
		viewColumn: vscode.ViewColumn.One,
		preserveFocus: false,
	});
	let changes = 0;
	const counting = vscode.workspace.onDidChangeTextDocument((event) => {
		if (event.document === longDocument) {
			changes += 1;
		}
	});
	const beforeBurst = exports.panelReport();
	const applied = [];
	for (let index = 0; index < 6; index += 1) {
		applied.push(
			await editor.edit((builder) =>
				builder.insert(new vscode.Position(0, 0), `Burst ${index}.\n\n`),
			),
		);
	}
	counting.dispose();
	assert.deepStrictEqual(
		applied,
		applied.map(() => true),
		`every keystroke was applied: ${JSON.stringify(applied)}`,
	);
	assert.ok(
		changes >= applied.length,
		`every edit changed the buffer: ${changes} changes for ${applied.length} edits`,
	);
	assert.strictEqual(
		longDocument.isDirty,
		true,
		"the buffer is unsaved, which is the case the requirement is about",
	);
	const expected = await probe.open("converged", longDocument.getText(), {
		path: long,
		settings,
	});
	let converged = exports.panelReport();
	for (let attempt = 0; attempt < 100; attempt += 1) {
		converged = exports.panelReport();
		if (
			Math.abs(converged.documentHeight - expected.height) <= 1 &&
            converged.paintedVersion === longDocument.version && !converged.refreshing &&
			converged.live >= 1
		) {
			break;
		}
		await delay(100);
	}
	assert.ok(
		Math.abs(converged.documentHeight - expected.height) <= 1,
		`the preview converged on the buffer: ${converged.documentHeight} against ${expected.height}`,
	);
	// Coalescing means fewer layouts than changes, and each layout is one
	// request to the engine: the panel opens a document once, not twice.
	const layouts = converged.opens - beforeBurst.opens;
	assert.ok(layouts >= 1, "the burst reached the engine");
	assert.ok(
		layouts < changes,
		`${changes} changes cost ${layouts} layouts, which is not coalescing`,
	);
	assert.ok(
		layouts <= 2,
		`six changes cost ${layouts} layouts, more than the coalescing allows`,
	);
	assert.ok(
		converged.documentHeight > beforeBurst.documentHeight,
		"the preview grew with the document",
	);
	say("burst", {
		edits: 6,
		changes,
		opens: layouts,
		height: Math.round(converged.documentHeight),
		expected: Math.round(expected.height),
	});

	async function testScrollSync() {
	// EXT-5: the surfaces follow each other, and neither moves the caret. An
	// edit holds both surfaces still for a moment, so the scroll comes after.
	await delay(600);
	const caret = editor.selection.active;
	// The bytes each surface is showing, converted by the test itself.
	const previewBytes = (document, report) =>
		Buffer.byteLength(document.getText().slice(0, report.source), "utf8");
	const editorBytes = (document, offset) =>
		Buffer.byteLength(document.getText().slice(0, offset), "utf8");
	const beforePreviewScroll = exports.panelReport();
	await scrollPreview(
		exports,
		converged.documentHeight * 0.6,
		// `revealRange` returns before the editor publishes its new viewport.
        (report) => report.syncs.preview > beforePreviewScroll.syncs.preview &&
            Math.abs(editorBytes(longDocument, longDocument.offsetAt(editor.visibleRanges[0].start)) -
                previewBytes(longDocument, report)) <= 400,
	);
	const followed = exports.panelReport();
	say("after-preview-scroll", {
		scroll: Math.round(followed.scroll),
		syncs: followed.syncs,
		stoodDown: followed.stoodDown,
		blocks: followed.blocks,
		scrollMessages: followed.scrollMessages,
		unmapped: followed.unmapped,
		editorEvents: followed.editorEvents,
		line: editor.visibleRanges[0].start.line,
	});
	assert.ok(
		followed.syncs.preview > beforePreviewScroll.syncs.preview,
		`the editor was moved by the preview: ${JSON.stringify(followed)}`,
	);
	// One move, and the moved surface did not move the other one back: the
	// answer to a carried scroll is not another carried scroll.
	assert.strictEqual(
		followed.syncs.editor,
		beforePreviewScroll.syncs.editor,
		"moving the editor back did not move the preview again",
	);
	assert.ok(
		editor.visibleRanges[0].start.line > 0,
		`the editor's view moved: ${editor.visibleRanges[0].start.line}`,
	);
	// The editor is at the line the preview's top source offset names, which
	// it could not be if either surface had confused bytes with code units.
	const editorAt = editorBytes(
		longDocument,
		longDocument.offsetAt(editor.visibleRanges[0].start),
	);
	const previewAt = previewBytes(longDocument, followed);
	assert.ok(
		Math.abs(editorAt - previewAt) <= 400,
		`the editor is at the line the preview shows: byte ${editorAt} against ${previewAt}`,
	);
	assert.ok(
		editor.selection.active.isEqual(caret),
		"the caret did not move with the view",
	);

	const beforeEditorScroll = exports.panelReport();
	const target = new vscode.Position(
		Math.floor(longDocument.lineCount * 0.8),
		0,
	);
	editor.revealRange(
		new vscode.Range(target, target),
		vscode.TextEditorRevealType.AtTop,
	);
	await delay(700);
	const answered = exports.panelReport();
	assert.ok(
		answered.syncs.editor > beforeEditorScroll.syncs.editor,
		"the preview was moved by the editor",
	);
	assert.strictEqual(
		answered.syncs.preview,
		beforeEditorScroll.syncs.preview,
		"moving the preview back did not move the editor again",
	);
	assert.ok(
		Math.abs(answered.scroll - beforeEditorScroll.scroll) > 100,
		`the preview's view moved: ${beforeEditorScroll.scroll} then ${answered.scroll}`,
	);
	assert.ok(
		editor.selection.active.isEqual(caret),
		"revealing a range did not move the caret",
	);
	assert.ok(
		Math.abs(
			previewBytes(longDocument, answered) -
				editorBytes(
					longDocument,
					longDocument.offsetAt(editor.visibleRanges[0].start),
				),
		) <= 400,
		`the preview is at the line the editor shows: byte ${previewBytes(longDocument, answered)}`,
	);

	// Neither surface keeps nudging the other once the move has been applied.
	const steady = exports.panelReport();
	await delay(1200);
	const still = exports.panelReport();
	assert.deepStrictEqual(
		still.syncs,
		steady.syncs,
		"the two surfaces settled instead of oscillating",
	);
	assert.ok(
		still.stoodDown > 0,
		"a surface stood down for the one that had just moved",
	);
	say("scroll-sync", {
		bytes: [
			editorBytes(
				longDocument,
				longDocument.offsetAt(editor.visibleRanges[0].start),
			),
			previewBytes(longDocument, still),
		],
		preview: still.syncs.preview,
		editor: still.syncs.editor,
		stoodDown: still.stoodDown,
		caret: editor.selection.active.line,
	});

	// NFR-3: typing does not move the reader's place. The edit is made above
	// the viewport, which is the case where the offset the reader was at no
	// longer holds the same text, and what has to hold is the text itself and
	// where in the viewport it sits.
	// One line of this document, in source units, as the slack a boundary
	// between two rows may cost.
	const ROW = 120;
	const beforeTyping = exports.panelReport();
	const inserted = "Inserted above.\n\n";
	const beforeSyncs = beforeTyping.syncs.editor;
	await editor.edit((builder) =>
		builder.insert(new vscode.Position(0, 0), inserted),
	);
	await delay(1500);
	const afterTyping = exports.panelReport();
	// The same text is at the top, to within a line: the view is held by the
	// row it is in, and a row boundary is a line.
	assert.ok(
		Math.abs(afterTyping.source - (beforeTyping.source + inserted.length)) <=
			ROW,
		`the same text is at the top of the view: ${beforeTyping.source} + ${inserted.length} then ${afterTyping.source}`,
	);
	assert.ok(
		Math.abs(afterTyping.into - beforeTyping.into) <= ROW,
		`and at the same place in the view: ${beforeTyping.into} then ${afterTyping.into}`,
	);
	assert.strictEqual(
		afterTyping.syncs.editor,
		beforeSyncs,
		"the edit did not carry the editor's own move into the preview",
	);
	say("typing", {
		inserted: inserted.length,
		source: [beforeTyping.source, afterTyping.source],
		into: [Math.round(beforeTyping.into), Math.round(afterTyping.into)],
		scroll: [Math.round(beforeTyping.scroll), Math.round(afterTyping.scroll)],
	});

	// An edit holds the surfaces still for a moment, but a scroll the reader
	// makes in that moment is theirs: it is carried once the edit has settled,
	// and the reader's anchor moves with it.
	const beforeHeld = exports.panelReport();
	const during = editor.edit((builder) =>
		builder.insert(new vscode.Position(0, 0), "During the hold.\n\n"),
	);
	await delay(150);
	assert.ok(
		exports.panelReport().syncs.preview === beforeHeld.syncs.preview,
		"the hold was in place when the scroll was made",
	);
	await exports.scrollPreviewTo(beforeHeld.documentHeight * 0.35);
	await during;
	await delay(1600);
	const afterHeld = exports.panelReport();
	assert.ok(
		afterHeld.syncs.preview > beforeHeld.syncs.preview,
		`the scroll made during the hold was carried when it settled: ${afterHeld.syncs.preview} against ${beforeHeld.syncs.preview}`,
	);
	const heldLine = editor.visibleRanges[0].start.line;
	assert.ok(
		heldLine > beforeHeld.scroll / 60,
		`the editor followed the scroll made during the hold: line ${heldLine}`,
	);

	// And the anchor the host holds is the place that scroll left it at, so an
	// insertion above keeps that same block at the top.
	const anchored = exports.panelReport();
	const heldInsert = "Anchored above.\n\n";
	await editor.edit((builder) =>
		builder.insert(new vscode.Position(0, 0), heldInsert),
	);
	await delay(1500);
	const kept = exports.panelReport();
	assert.ok(
		Math.abs(kept.source - (anchored.source + heldInsert.length)) <= ROW,
		`the text the held scroll reached is still at the top: ${anchored.source} + ${heldInsert.length} then ${kept.source}`,
	);
	assert.ok(
		Math.abs(kept.into - anchored.into) <= ROW,
		`and at the same place in the view: ${anchored.into} then ${kept.into}`,
	);
	say("held-scroll", {
		syncs: [beforeHeld.syncs.preview, afterHeld.syncs.preview],
		line: heldLine,
		source: [anchored.source, kept.source],
		into: [Math.round(anchored.into), Math.round(kept.into)],
	});

	// A scroll a few pixels into a block leaves the reader part-way through
	// it, and both halves of the anchor come from the same report, so typing
	// above cannot turn ten pixels into none.
	const partWay = exports.panelReport();
	const wanted = partWay.scroll - partWay.into + 10;

	await exports.scrollPreviewTo(wanted);
	await delay(500);
	const scrolled = exports.panelReport();
	assert.ok(
		Math.abs(scrolled.into - 10) <= scrollPixel(scrolled),
		`the view is ten pixels into a row: ${scrolled.into}`,
	);
	await editor.edit((builder) =>
		builder.insert(new vscode.Position(0, 0), "Part way.\n\n"),
	);
	await delay(1500);
	const stayed = exports.panelReport();
	assert.ok(
		Math.abs(stayed.into - scrolled.into) <= Math.max(scrollPixel(scrolled), scrollPixel(stayed)),
		`the reader is still part-way into the same row: ${scrolled.into} then ${stayed.into}`,
	);
	assert.ok(
		Math.abs(stayed.source - (scrolled.source + "Part way.\n\n".length)) <=
			ROW,
		"and it is the same text",
	);
	say("part-way", {
		into: [Math.round(scrolled.into * 100) / 100, Math.round(stayed.into * 100) / 100],
		source: [scrolled.source, stayed.source],
	});

	// EXT-5 and NFR-3 inside a block of many screens: a fenced block is one
	// block however many lines it has, so a position inside it is a row, and
	// its lines are a few bytes each, so a count of bytes is not a distance.
	const code = path.join(scratch, "code.md");
	const lines = Array.from({ length: 500 }, (_, index) => `x${index}`).join("\n");
	fs.writeFileSync(code, ['# Code', '', '```text', lines, '```', '', 'Tail.', ''].join('\n'));
	const codeDocument = await vscode.workspace.openTextDocument(code);
	await vscode.commands.executeCommand("workbench.action.closeAllEditors");
	await delay(300);
	const codeEditor = await vscode.window.showTextDocument(codeDocument, {
		viewColumn: vscode.ViewColumn.One,
		preserveFocus: false,
		preview: false,
	});
	await delay(400);
	await vscode.commands.executeCommand("markview.openPreview", codeDocument.uri);
	let codeReport = exports.panelReport();
	for (let attempt = 0; attempt < 100; attempt += 1) {
		codeReport = exports.panelReport();
		if (codeReport.documentHeight > 5000 && codeReport.live >= 1) {
			break;
		}
		await delay(100);
	}
	assert.ok(
		codeReport.documentHeight > 5000,
		`the code block is many screens tall: ${codeReport.documentHeight}`,
	);

	// The editor is sent to a line deep inside the block, and the preview
	// follows it there rather than to the top of the block.
	const deep = new vscode.Position(400, 0);
	const deepByte = codeDocument.offsetAt(deep);
	codeEditor.revealRange(
		new vscode.Range(deep, deep),
		vscode.TextEditorRevealType.AtTop,
	);
	await delay(1200);
	const followedDeep = exports.panelReport();
	assert.ok(
		Math.abs(followedDeep.source - deepByte) <= 60,
		`the preview is deep inside the block: source ${followedDeep.source} against ${deepByte}`,
	);
	assert.ok(
		followedDeep.scroll > 1000,
		`and not at the block start: ${followedDeep.scroll}`,
	);

	// A screenful of short lines is a real move: the preview follows the
	// editor line by line rather than treating the whole block as one place.
	const beforeShort = exports.panelReport();
	const short = new vscode.Position(
		codeDocument.lineCount - 60,
		0,
	);
	codeEditor.revealRange(
		new vscode.Range(short, short),
		vscode.TextEditorRevealType.AtTop,
	);
	await delay(1200);
	const afterShort = exports.panelReport();
	assert.ok(
		afterShort.syncs.editor > beforeShort.syncs.editor,
		"the editor's move was carried",
	);
	assert.ok(
		Math.abs(afterShort.scroll - beforeShort.scroll) > 500,
		`the preview moved a long way in pixels: ${beforeShort.scroll} then ${afterShort.scroll}`,
	);

	// Typing above the reader inside the block leaves the same line on screen.
	const anchoredCode = exports.panelReport();
	const typedLine = "inserted line\n";
	await codeEditor.edit((builder) =>
		builder.insert(new vscode.Position(6, 0), typedLine),
	);
	await delay(1600);
	const keptCode = exports.panelReport();
	assert.ok(
		Math.abs(keptCode.source - (anchoredCode.source + typedLine.length)) <= 60,
		`the same line is at the top of the block: ${anchoredCode.source} + ${typedLine.length} then ${keptCode.source}`,
	);
	assert.ok(
		keptCode.scroll > 500,
		`and the preview stayed in the block: ${keptCode.scroll}`,
	);
	say("code-block", {
		deep: [deepByte, followedDeep.source, Math.round(followedDeep.scroll)],
		short: [
			Math.round(beforeShort.scroll),
			Math.round(afterShort.scroll),
			beforeShort.syncs.editor,
			afterShort.syncs.editor,
		],
		typed: [anchoredCode.source, keptCode.source, Math.round(keptCode.scroll)],
	});
	// The block that a proportional search cannot handle: one line holding most
	// of its bytes, then thousands of short ones. The position of a line is
	// found from the engine's own rows, not from a share of the block.
	const uneven = path.join(scratch, "uneven.md");
	const many = Array.from({ length: 2000 }, (_, index) =>
		index === 100 || index === 200 ? "z".repeat(30000) : `y${index}`,
	).join("\n");
	fs.writeFileSync(
		uneven,
		[
			"# Uneven",
			"",
			"```text",
			"short",
			many,
			"```",
			"",
			"Tail.",
			"",
		].join("\n"),
	);
	const unevenDocument = await vscode.workspace.openTextDocument(uneven);
	await vscode.commands.executeCommand("workbench.action.closeAllEditors");
	await delay(300);
	const unevenEditor = await vscode.window.showTextDocument(unevenDocument, {
		viewColumn: vscode.ViewColumn.One,
		preserveFocus: false,
		preview: false,
	});
	await delay(400);
	await vscode.commands.executeCommand("markview.openPreview", unevenDocument.uri);
	let unevenReport = exports.panelReport();
	for (let attempt = 0; attempt < 100; attempt += 1) {
		unevenReport = exports.panelReport();
		if (unevenReport.documentHeight > 10000 && unevenReport.live >= 1) {
			break;
		}
		await delay(100);
	}
	assert.ok(
		unevenReport.documentHeight > 10000,
		`the uneven block is many screens tall: ${unevenReport.documentHeight}`,
	);

	const unevenTarget = new vscode.Position(1500, 0);
	const unevenByte = unevenDocument.offsetAt(unevenTarget);
	unevenEditor.revealRange(
		new vscode.Range(unevenTarget, unevenTarget),
		vscode.TextEditorRevealType.AtTop,
	);
	await delay(1500);
	const unevenFollowed = exports.panelReport();
	assert.ok(
		Math.abs(unevenFollowed.source - unevenByte) <= 60,
		`the preview found the line below the long one: ${unevenFollowed.source} against ${unevenByte}`,
	);

	const unevenAnchored = exports.panelReport();
	const unevenLine = "inserted\n";
	await unevenEditor.edit((builder) =>
		builder.insert(new vscode.Position(10, 0), unevenLine),
	);
	await delay(1600);
	const unevenKept = exports.panelReport();
	assert.ok(
		Math.abs(unevenKept.source - (unevenAnchored.source + unevenLine.length)) <=
			60,
		`and kept it across an edit above: ${unevenAnchored.source} + ${unevenLine.length} then ${unevenKept.source}`,
	);
	say("uneven-block", {
		deep: [unevenByte, unevenFollowed.source, Math.round(unevenFollowed.scroll)],
		typed: [
			unevenAnchored.source,
			unevenKept.source,
			Math.round(unevenKept.scroll),
		],
	});

	// Six insertions in a row, each left to settle. Every one of them must move
	// the reader's row down by exactly what it inserted: a restore that the
	// editor then steers back, or an anchor that does not follow an edit, shows
	// up here as a reader drifting backwards one line at a time.
	const repeats = [];
	let expectedSource = unevenKept.source;
	for (let index = 0; index < 6; index += 1) {
		await unevenEditor.edit((builder) =>
			builder.insert(new vscode.Position(10, 0), unevenLine),
		);
		await delay(1400);
		const at = exports.panelReport();
		expectedSource += unevenLine.length;
		repeats.push({
			expected: expectedSource,
			source: at.source,
			scroll: Math.round(at.scroll),
		});
	}
	say("repeated-insertions", repeats);
	// The reader's place is a byte, and the report names the row that byte is
	// drawn at. A row boundary is quantized, so a re-wrap can put the byte in
	// a neighbouring short row — this document has a six-byte one — but the
	// row must not drift: an anchor that ignores an edit, or a restore the
	// editor then steers back, slips a whole line on every insertion.
	const slack = 8;
	let previousSource = unevenKept.source;
	for (const step of repeats) {
		assert.ok(
			Math.abs(step.source - previousSource - unevenLine.length) <= slack,
			`the reader's row followed the insertions: ${previousSource} + ${unevenLine.length} then ${step.source}`,
		);
		previousSource = step.source;
	}
	const advanced = previousSource - unevenKept.source;
	assert.ok(
		Math.abs(advanced - unevenLine.length * repeats.length) <= slack,
		`the reader's row did not drift over six insertions: ${advanced} against ${unevenLine.length * repeats.length}`,
	);

	// A reader who moves the editor themselves just after the panel revealed a
	// line must still be followed: the answer to the panel's reveal is one
	// report, and the next one is the reader's.
	await exports.scrollPreviewTo(exports.panelReport().documentHeight * 0.3);
	await delay(200);
	const earlyBefore = exports.panelReport();
	const earlyLine = unevenEditor.visibleRanges[0].start.line;
	say("landing", {
		asked: earlyBefore.revealed,
		askedLine: unevenDocument.positionAt(
			Math.max(0, earlyBefore.revealed),
		).line,
		topLine: earlyLine,
		firstStart: unevenDocument.offsetAt(
			unevenEditor.visibleRanges[0].start,
		),
		askedOffset: earlyBefore.revealed,
	});
	const earlyTarget = new vscode.Position(1500, 0);
	unevenEditor.revealRange(
		new vscode.Range(earlyTarget, earlyTarget),
		vscode.TextEditorRevealType.AtTop,
	);
	await delay(1000);
	const earlyAfter = exports.panelReport();
	assert.ok(
		unevenEditor.visibleRanges[0].start.line !== earlyLine,
		`the editor moved: line ${earlyLine} then ${unevenEditor.visibleRanges[0].start.line}`,
	);
	assert.ok(
		earlyAfter.syncs.editor > earlyBefore.syncs.editor,
		"the reader's own move was carried",
	);
	assert.ok(
		Math.abs(earlyAfter.scroll - earlyBefore.scroll) > 400,
		`and the preview followed it: ${earlyBefore.scroll} then ${earlyAfter.scroll}`,
	);
	say("early-editor-move", {
		lines: [earlyLine, unevenEditor.visibleRanges[0].start.line],
		scroll: [Math.round(earlyBefore.scroll), Math.round(earlyAfter.scroll)],
		editor: [earlyBefore.syncs.editor, earlyAfter.syncs.editor],
	});

	// A reveal that moves nothing must not leave the panel waiting for an
	// answer: a one-pixel scroll on the same row reveals the row the editor is
	// already showing, so no report follows, and the reader's next move is
	// theirs.
	await exports.scrollPreviewTo(exports.panelReport().documentHeight * 0.3);
	await delay(1000);
	await exports.scrollPreviewTo(exports.panelReport().scroll + 1);
	await delay(50);
	const noopBefore = exports.panelReport();
	unevenEditor.revealRange(
		new vscode.Range(
			new vscode.Position(1500, 0),
			new vscode.Position(1500, 0),
		),
		vscode.TextEditorRevealType.AtTop,
	);
	await delay(1000);
	const noopAfter = exports.panelReport();
	assert.ok(
		noopAfter.syncs.editor > noopBefore.syncs.editor,
		"a move after a reveal that moved nothing was carried",
	);
	assert.ok(
		Math.abs(noopAfter.scroll - noopBefore.scroll) > 400,
		`and the preview followed it: ${noopBefore.scroll} then ${noopAfter.scroll}`,
	);
	say("no-op-reveal-then-move", {
		scroll: [Math.round(noopBefore.scroll), Math.round(noopAfter.scroll)],
		editor: [noopBefore.syncs.editor, noopAfter.syncs.editor],
	});

	// A reader who moves a few lines while the byte the panel asked for is
	// still on screen must be followed: the byte being visible is not what
	// makes a report the answer to a reveal the panel never needed to make.
	const overlapBefore = exports.panelReport();
	unevenEditor.revealRange(
		new vscode.Range(
			new vscode.Position(
				Math.max(0, unevenEditor.visibleRanges[0].start.line - 8),
				0,
			),
			new vscode.Position(
				Math.max(0, unevenEditor.visibleRanges[0].start.line - 8),
				0,
			),
		),
		vscode.TextEditorRevealType.AtTop,
	);
	await delay(1000);
	const overlapAfter = exports.panelReport();
	assert.ok(
		overlapAfter.syncs.editor > overlapBefore.syncs.editor,
		"a move that left the asked-for byte on screen was carried",
	);
	assert.ok(
		Math.abs(overlapAfter.scroll - overlapBefore.scroll) > 100,
		`and the preview followed it: ${overlapBefore.scroll} then ${overlapAfter.scroll}`,
	);
	say("overlapping-move", {
		lines: [
			overlapBefore.revealed,
			unevenEditor.visibleRanges[0].start.line,
		],
		scroll: [Math.round(overlapBefore.scroll), Math.round(overlapAfter.scroll)],
	});

	// Returning to the line the panel revealed is the reader's move too: the
	// landing the panel remembered must not still be remembered after a move
	// was carried, or the return is mistaken for the answer arriving twice.
	const backBefore = exports.panelReport();
	unevenEditor.revealRange(
		new vscode.Range(
			new vscode.Position(earlyLine, 0),
			new vscode.Position(earlyLine, 0),
		),
		vscode.TextEditorRevealType.AtTop,
	);
	await delay(1000);
	const backAfter = exports.panelReport();
	assert.ok(
		backAfter.syncs.editor > backBefore.syncs.editor,
		"the return to the panel's own landing was carried",
	);
	assert.ok(
		Math.abs(backAfter.scroll - earlyBefore.scroll) < 600,
		`the preview followed the editor back: ${earlyBefore.scroll} then ${backAfter.scroll}`,
	);
	say("return-to-landing", {
		line: earlyLine,
		scroll: [Math.round(earlyBefore.scroll), Math.round(backAfter.scroll)],
		editor: [backBefore.syncs.editor, backAfter.syncs.editor],
	});

	// The same insertions at the largest supported text size. The editor's
	// landing is then half a screen from the line it is asked to show, which
	// is why the answer to a reveal is recognised rather than measured.
	await vscode.workspace
		.getConfiguration("markview")
		.update("fontSize", 48, vscode.ConfigurationTarget.Global);
	await vscode.commands.executeCommand("markview.closePreview");
	await delay(400);
	await vscode.commands.executeCommand("markview.openPreview", unevenDocument.uri);
	let bigReport = exports.panelReport();
	for (let attempt = 0; attempt < 100; attempt += 1) {
		bigReport = exports.panelReport();
		if (bigReport.documentHeight > 10000 && bigReport.live >= 1) {
			break;
		}
		await delay(100);
	}
	await exports.scrollPreviewTo(bigReport.documentHeight * 0.6);
	await delay(900);
	const bigRepeats = [];
	let bigExpected = exports.panelReport().source;
	for (let index = 0; index < 4; index += 1) {
		await unevenEditor.edit((builder) =>
			builder.insert(new vscode.Position(10, 0), unevenLine),
		);
		await delay(1400);
		const at = exports.panelReport();
		bigExpected += unevenLine.length;
		bigRepeats.push({ expected: bigExpected, source: at.source });
	}
	say("large-font-insertions", bigRepeats);
	for (const step of bigRepeats) {
		assert.ok(
			Math.abs(step.source - step.expected) <= 4,
			`the reader's row followed the insertions at 48 points: expected ${step.expected}, at ${step.source}`,
		);
	}
	await vscode.workspace
		.getConfiguration("markview")
		.update("fontSize", undefined, vscode.ConfigurationTarget.Global);
	await delay(300);

	// An edit that reaches across the reader: the text they were on is
	// replaced, so the nearest survivor is the end of what replaced it. An
	// anchor that only followed edits entirely before it would stay where it
	// was and lose the reader by the length of the replacement.
	const beforeReplace = exports.panelReport();
	const replaceFrom = beforeReplace.source - 300;
	await unevenEditor.edit((builder) =>
		builder.replace(
			new vscode.Range(
				unevenDocument.positionAt(replaceFrom),
				unevenDocument.positionAt(beforeReplace.source + 1),
			),
			"R",
		),
	);
	await delay(1400);
	const afterReplace = exports.panelReport();
	assert.ok(
		Math.abs(afterReplace.source - (replaceFrom + 1)) <= 12,
		`the reader followed the edit that replaced their text: ${afterReplace.source} against ${replaceFrom + 1}`,
	);
	say("straddling-replace", {
		before: beforeReplace.source,
		from: replaceFrom,
		after: afterReplace.source,
	});
	}
	if (process.env.MARKVIEW_SKIP_SCROLL_SYNC === "1") {
		say("SKIP EXT-5", "scroll synchronization and edit anchors deferred; not validated");
	} else {
		await testScrollSync();
	}
	// EXT-6, EXT-7, EXT-11: the reader points at the preview and the editor
	// answers. Every rectangle the test clicks comes from the engine's own
	// answer for the same text, so a click lands where the engine drew it.
	const linked = path.join(scratch, "linked.md");
	const other = path.join(scratch, "other.md");
	// A target the engine asks about rather than opening: the extension is not
	// on the inert list, so handing it to the desktop would run it.
	const scriptPath = path.join(scratch, "script.sh");
	fs.writeFileSync(scriptPath, "#!/bin/sh\necho hello\n");
	fs.writeFileSync(other, "# Other\n\nAnother document.\n");
	fs.writeFileSync(
		linked,
		[
			"# Linked",
			"",
			"A [local document](other.md), a [remote address](https://example.com/x), and a [script](script.sh).",
			"",
			"## A heading",
			"",
			// The fragment is above this filler, so following it is a real
			// scroll rather than a document that fits its viewport.
			...Array.from(
				{ length: 60 },
				(_, index) => `Filler paragraph ${index} below the heading.`,
			),
			"",
			"Back to [the heading](#a-heading).",
			"",
		].join("\n"),
	);
	const linkedDocument = await vscode.workspace.openTextDocument(linked);
	await vscode.commands.executeCommand("workbench.action.closeAllEditors");
	await delay(300);
	await vscode.window.showTextDocument(linkedDocument, {
		viewColumn: vscode.ViewColumn.One,
		preserveFocus: false,
		preview: false,
	});
	await delay(400);
	await vscode.commands.executeCommand("markview.openPreview", linkedDocument.uri);
	let linkedReport = exports.panelReport();
	for (let attempt = 0; attempt < 100; attempt += 1) {
		linkedReport = exports.panelReport();
		if (linkedReport.documentHeight > 200 && linkedReport.live >= 1) {
			break;
		}
		await delay(100);
	}
	assert.ok(linkedReport.live >= 1, "the linked document is drawn");

	// The engine's own answer for the same text: its block map with the links
	// it classified, its text layer, and where it put each fragment.
	const linkedState = await probe.open("linked", linkedDocument.getText(), {
		path: linked,
		settings,
	});
	const linkMap = [];
	for (const block of linkedState.blocks) {
		for (const link of block.links ?? []) {
			linkMap.push({ ...link, pageY: block.y + link.y });
		}
	}
	const linkedLayer = await probe.text("linked", 0, linkedState.height);
	const fragment = linkMap.find((link) => link.kind === "anchor");
	assert.ok(fragment, "the engine classified the fragment");
	assert.strictEqual(typeof fragment.to, "number", "and resolved it");

	// The preview's viewport is short, so a link below the fold is brought
	// into it before it is clicked: a click is dispatched at the point a
	// reader would have to see.
	const clickLink = async (link) => {
		// The engine's own links carry a page position; the panel's carry the
		// block-relative one it hit-tests with.
		const pageY = link.pageY ?? link.y;
		await exports.scrollPreviewTo(Math.max(0, pageY - 40));
		await delay(400);
		// A click that ends a selection is the reader selecting rather than
		// pointing, and the steps before this one leave a selection behind.
		await exports.clearPreviewSelection();
		await delay(150);
		await exports.clickPreviewAt(
			link.x + link.width / 2,
			pageY + link.height / 2,
		);
	};

	// Exercise the desktop handoff; Linux records `xdg-open` instead of launching a browser.
	const remote = linkMap.find((link) => link.kind === "remote");
	assert.ok(
		remote,
		`the engine classified the remote link: ${JSON.stringify(linkMap)}`,
	);
	await clickLink(remote);
	await delay(700);
	const routed = exports.panelReport().routed;
	assert.strictEqual(routed.length, 1, "the click was routed once");
	assert.strictEqual(routed[0].kind, "remote");
	assert.ok(
		routed[0].target.includes("example.com"),
		`to the address the engine classified: ${JSON.stringify(routed)}`,
	);

	if (process.env.MARKVIEW_EXTERNAL_LOG) {
		const handedOff = () => fs.existsSync(process.env.MARKVIEW_EXTERNAL_LOG) &&
			fs.readFileSync(process.env.MARKVIEW_EXTERNAL_LOG, "utf8").split("\n").includes(remote.target);
		for (let attempt = 0; attempt < 50 && !handedOff(); attempt++) await delay(100);
		assert.ok(handedOff(), "the real desktop opener receives the remote URL");
	}

	// A local document is opened in the editor.
	const local = linkMap.find((link) => link.kind === "document");
	assert.ok(local, "the engine classified the local link");
	await clickLink(local);
	// The temporary directory is under `/var`, which is a link to
	// `/private/var`, so the editor reports the real path.
	const otherReal = fs.realpathSync(other);
	for (let attempt = 0; attempt < 30; attempt += 1) {
		if (vscode.window.activeTextEditor?.document.uri.fsPath === otherReal) {
			break;
		}
		await delay(100);
	}
	assert.strictEqual(
		vscode.window.activeTextEditor?.document.uri.fsPath,
		otherReal,
		`the local link opened in the editor: ${JSON.stringify(exports.panelReport().routed)}`,
	);

    // Following the local link also switches preview; return to test its other links.
    await vscode.window.showTextDocument(linkedDocument, vscode.ViewColumn.One);
    for (let attempt = 0; attempt < 100; attempt++) {
        const report = exports.panelReport();
        if (report.documentUri === linkedDocument.uri.toString() && report.paintedVersion === linkedDocument.version) break;
        await delay(100);
    }
    assert.strictEqual(exports.panelReport().documentUri, linkedDocument.uri.toString());

	// A local target the engine would not open by itself — a script, which
	// the desktop would run — is confirmed rather than handed over. The test
	// host refuses dialogs, so a route that asked before opening leaves the
	// editor alone; nothing may open it behind the reader's back.
	const script = linkMap.find((link) => link.kind === "confirm");
	assert.ok(script, `the engine asked for confirmation: ${JSON.stringify(linkMap)}`);
	await clickLink(script);
	await delay(900);
	const asked = exports.panelReport().routed.at(-1);
	assert.strictEqual(asked?.kind, "confirm", JSON.stringify(asked));
	// The temporary directory is under `/var`, which is a link to
	// `/private/var`, so the engine reports the real path.
	const scriptReal = fs.realpathSync(scriptPath);
	assert.strictEqual(asked?.target, scriptReal, `${JSON.stringify(asked)}`);
	assert.ok(
		!vscode.window.visibleTextEditors.some(
			(editor) => editor.document.uri.fsPath === scriptReal,
		),
		"an unconfirmed target was not opened",
	);
	say("confirm", { target: asked?.target });

	// A fragment is a place in this document the engine resolved. The link
	// sits under the filler, so the view is taken to it before it is clicked,
	// at the rectangle the panel itself hit-tests against.
	const panelFragment = () =>
		exports.panelReport().links.find((link) => link.kind === "anchor");
	assert.ok(panelFragment(), "the panel drew the fragment link");
	await clickLink(panelFragment());
	await delay(800);
	assert.ok(
		Math.abs(exports.panelReport().scroll - fragment.to) <= 2,
		`the fragment scrolled to where the engine resolved it: ${exports.panelReport().scroll} against ${fragment.to}`,
	);

	// The characters the find widget searches are the ones the engine drew,
	// in reading order and one per source offset — not a summary of them. The
	// layer is drawn a band at a time, and the bands arrive in the order the
	// reader scrolls rather than the order of the text.
	const drawn = exports.panelReport().text;
	const drawnByEngine = linkedLayer.clusters
		.map((item) => String(item.text))
		.join("");
	assert.strictEqual(
		drawn,
		drawnByEngine,
		"the text layer holds the characters the engine drew",
	);
	assert.strictEqual(
		exports.panelReport().ordered,
		true,
		"the drawn spans are in reading order",
	);
	assert.strictEqual(
		exports.panelReport().unique,
		true,
		"each source offset is drawn once",
	);

	// Clicking text reveals the bytes under the point in the editor. The
	// source is deliberately not the editor on screen: the preview shows it
	// rather than dropping the click.
	const cluster = linkedLayer.clusters.find(
		(item) =>
			String(item.text).trim() !== "" &&
			item.source_end > item.source_start &&
			item.y < 400 &&
			linkedDocument.getText().slice(item.source_start, item.source_end) ===
				String(item.text),
	);
	assert.ok(
		cluster,
		"the engine drew a cluster whose text is the document's own",
	);
	await vscode.commands.executeCommand("workbench.action.closeAllEditors");
	await delay(400);
	await vscode.window.showTextDocument(await vscode.workspace.openTextDocument(other), {
		viewColumn: vscode.ViewColumn.One,
		preserveFocus: false,
		preview: false,
	});
	await delay(400);
	// The preview is opened for the document while no editor is showing it:
	// that is the case the click has to handle.
	await vscode.commands.executeCommand("markview.openPreview", linkedDocument.uri);
	for (let attempt = 0; attempt < 60; attempt += 1) {
		if (exports.panelReport().live >= 1) {
			break;
		}
		await delay(100);
	}
	assert.ok(
		!vscode.window.visibleTextEditors.some(
			(editor) => editor.document.uri.fsPath === linked,
		),
		"the source of the preview is not the editor on screen",
	);
	await clickLink({
		...cluster,
		pageY: cluster.y,
	});
	await delay(900);
	const clicked = vscode.window.visibleTextEditors.find(
		(editor) => editor.document.uri.fsPath === linked,
	);
	assert.ok(clicked, "the click showed the source document");
	const selectedStart = clicked.document.offsetAt(clicked.selection.start);
	const selectedEnd = clicked.document.offsetAt(clicked.selection.end);
	// The click reveals the block holding the bytes under the point, so the
	// selection is that block and must contain them.
	assert.ok(
		!clicked.selection.isEmpty &&
			selectedStart <= cluster.source_start &&
			cluster.source_end <= selectedEnd,
		`the click selected the block holding the bytes under it: ${selectedStart}..${selectedEnd} against ${cluster.source_start}..${cluster.source_end}`,
	);
	say("click-source", {
		document: clicked.document.uri.fsPath.split("/").pop(),
		selected: [selectedStart, selectedEnd],
		cluster: [cluster.source_start, cluster.source_end],
	});

	// A drag over the rendered content selects the text layer's own
	// characters, and copying takes them: both are the browser's own
	// selection over the spans, which is what a reader's pointer does.
	const line = linkedLayer.clusters
		.filter(
			(item) =>
				item.y === cluster.y &&
				item.source_end > item.source_start &&
				linkedDocument.getText().slice(item.source_start, item.source_end) ===
					String(item.text),
		)
		.sort((left, right) => left.source_start - right.source_start);
	const dragFrom = line[0];
	const dragTo = line[Math.min(4, line.length - 1)];
	assert.ok(dragTo && dragTo !== dragFrom, "the engine drew a line to drag across");
	await exports.dragPreviewAt(
		{ x: dragFrom.x + 1, y: dragFrom.y + dragFrom.height / 2 },
		{ x: dragTo.x + dragTo.width - 1, y: dragTo.y + dragTo.height / 2 },
	);
	await delay(600);
	const dragged = exports.panelReport().selection;
	assert.ok(dragged, "the drag selected text in the preview");
	const across = linkedDocument
		.getText()
		.slice(dragFrom.source_start, dragTo.source_end);
	assert.strictEqual(
		dragged.text,
		across,
		`the drag selected the document's own characters: ${JSON.stringify(dragged.text)} against ${JSON.stringify(across)}`,
	);
	// The host command copies the current native browser selection.
	const previousClipboard = await vscode.env.clipboard.readText();
	await vscode.env.clipboard.writeText("");
	await vscode.commands.executeCommand("markview.copySelection");
	await delay(600);
	const clipboard = await vscode.env.clipboard.readText();
	await vscode.env.clipboard.writeText(previousClipboard);
	const copied = exports.panelReport();
	say("select-copy", {
		selected: [dragFrom.source_start, dragTo.source_end],
		length: dragged.text.length,
		clipboard: clipboard.length,
		copied: copied.copied,
		focused: copied.focused,
		error: copied.copyError,
	});
	assert.ok(copied.copied, `host copy failed: ${copied.copyError}`);
	assert.strictEqual(clipboard, across, "the clipboard contains the selected text");

	// A match that ends at the last drawn character has no cluster after it
	// to ask for, and still has to be followed: the last paragraph of this
	// document is what the rendered text ends with.
	// The rendered text says "Back to the heading." where the source says
	// "Back to [the heading](#a-heading).".
	const tail = "Back to the heading.";
	const tailAtSource = linkedDocument
		.getText()
		.indexOf("Back to [the heading](#a-heading).");
	await exports.findInPreview(tail);
	await delay(900);
	assert.strictEqual(
		exports.panelReport().revealed,
		tailAtSource,
		`a match ending at the end of the text was followed: ${exports.panelReport().revealed} against ${tailAtSource}`,
	);

	// What the reader selects in the editor is selected in the preview, which
	// is the text the find widget searches. The band carrying it may have to
	// be drawn first, and the mirror waits for it rather than selecting
	// whatever span is nearest.
	const mirroredText = linkedDocument.getText().slice(
		cluster.source_start,
		cluster.source_end,
	);
	// The selection is made in the editor the reader is looking at, which the
	// reveal above may have shown in another group.
	const mirrorEditor = await vscode.window.showTextDocument(linkedDocument, {
		viewColumn: vscode.ViewColumn.One,
		preserveFocus: false,
		preview: false,
	});
	mirrorEditor.selection = new vscode.Selection(
		mirrorEditor.document.positionAt(cluster.source_start),
		mirrorEditor.document.positionAt(cluster.source_end),
	);
	let mirrored = exports.panelReport().selection;
	for (let attempt = 0; attempt < 30; attempt += 1) {
		mirrored = exports.panelReport().selection;
		if (mirrored?.text === mirroredText) {
			break;
		}
		await exports.scrollPreviewTo(cluster.y - 60);
		await delay(150);
	}
	assert.ok(mirrored, "the preview reported a selection");
	say("mirror", {
		edited: [
			mirrorEditor.document.offsetAt(mirrorEditor.selection.start),
			mirrorEditor.document.offsetAt(mirrorEditor.selection.end),
		],
		reported: mirrored?.text,
		spans: exports.panelReport().spans,
		drawn: exports.panelReport().text.length,
	});
	assert.strictEqual(
		mirrored.text,
		mirroredText,
		"the preview selected the document's own characters",
	);

	// The selection survives scrolling: the drawn text is added to, never
	// rebuilt, so scrolling cannot take the nodes a selection points at away.
	await exports.scrollPreviewTo(linkedReport.documentHeight);
	await delay(700);
	assert.strictEqual(
		exports.panelReport().selection?.text,
		mirrored.text,
		"the selection survived scrolling to the end of the document",
	);
	await exports.scrollPreviewTo(0);
	await delay(500);

	// A selection in a part of the document whose band has not been drawn yet
	// is kept until it arrives: applying it to whatever span is nearest would
	// show the reader text they did not select, and nothing would repair it.
	const far = linkedLayer.clusters.filter(
		(item) => item.source_end > item.source_start && String(item.text).trim() !== "",
	).at(-1);
	assert.ok(far, "the document has a cluster at its end");
	const farText = linkedDocument.getText().slice(far.source_start, far.source_end);
	mirrorEditor.selection = new vscode.Selection(
		mirrorEditor.document.positionAt(far.source_start),
		mirrorEditor.document.positionAt(far.source_end),
	);
	await delay(500);
	await exports.scrollPreviewTo(far.y - 60);
	for (let attempt = 0; attempt < 40; attempt += 1) {
		if (exports.panelReport().selection?.text === farText) {
			break;
		}
		await delay(100);
	}
	assert.strictEqual(
		exports.panelReport().selection?.text,
		farText,
		`a selection in an undrawn part of the document was applied when its band arrived: ${JSON.stringify({request: exports.panelReport().selectionRequest, state: exports.panelReport().selectionState, far, scroll: exports.panelReport().scroll, requested: exports.panelReport().requested.slice(-5)})}`,
	);

	// A fragment follows the document: an edit above the heading moves it, and
	// the link has to reach the heading's new place rather than the one it had
	// when the link was first drawn. The edit is applied to the document
	// rather than to whichever editor holds the focus in this window.
	const heading = new vscode.WorkspaceEdit();
	heading.insert(
		linkedDocument.uri,
		new vscode.Position(0, 0),
		"Moved down.\n\n",
	);
	const movedIn = await vscode.workspace.applyEdit(heading);
	assert.strictEqual(movedIn, true, "the edit was applied");
	await delay(1400);
	const movedState = await probe.open(
		"linked-moved",
		linkedDocument.getText(),
		{ path: linked, settings },
	);
	const movedAnchor = [];
	for (const block of movedState.blocks) {
		for (const link of block.links ?? []) {
			if (link.kind === "anchor") {
				movedAnchor.push({ ...link, pageY: block.y + link.y });
			}
		}
	}
	assert.strictEqual(movedAnchor.length, 1, "the fragment is still there");
	assert.ok(
		movedAnchor[0].to > fragment.to,
		`the heading moved down: ${movedAnchor[0].to} against ${fragment.to}`,
	);
	// The fragment has to reach the heading the link names. Scroll sync is a
	// separate requirement and would let the editor's own view move the
	// preview afterwards, so it is off for this click.
	const syncSetting = vscode.workspace.getConfiguration("markview");
	await syncSetting.update(
		"scrollSync",
		false,
		vscode.ConfigurationTarget.Global,
	);
	await delay(200);
	const movedLink = panelFragment();
	assert.strictEqual(
		movedLink?.to,
		movedAnchor[0].to,
		`the panel followed the moved heading: ${JSON.stringify(movedLink)} against ${movedAnchor[0].to}`,
	);
	await clickLink(movedLink);
	await delay(800);
	const movedScroll = exports.panelReport().scroll;
	say("moved-click", {
		resolved: exports.panelReport().clicked,
		wanted: movedLink?.url,
		scroll: Math.round(movedScroll),
	});
	await syncSetting.update(
		"scrollSync",
		undefined,
		vscode.ConfigurationTarget.Global,
	);
	const movedReport = exports.panelReport();
	say("fragment-moved", {
		to: Math.round(movedAnchor[0].to),
		pageY: Math.round(movedAnchor[0].pageY),
		landed: Math.round(movedScroll),
		anchors: movedReport.anchors,
		routed: movedReport.routed.at(-1),
		carried: movedReport.carried,
		revealed: movedReport.revealed,
		syncs: movedReport.syncs,
	});
	assert.ok(
		Math.abs(movedScroll - movedAnchor[0].to) <= 2,
		`the fragment followed the heading: ${movedScroll} against ${movedAnchor[0].to}`,
	);

	// The find widget searches the document's own text, so a phrase the reader
	// has never scrolled to can be found from the top of a long document.
	await vscode.commands.executeCommand("markview.closePreview");
	await delay(300);
	await vscode.window.showTextDocument(longDocument, {
		viewColumn: vscode.ViewColumn.One,
		preserveFocus: false,
		preview: false,
	});
	await delay(400);
	await vscode.commands.executeCommand("markview.openPreview", longDocument.uri);
	let farReport = exports.panelReport();
	for (let attempt = 0; attempt < 100; attempt += 1) {
		farReport = exports.panelReport();
		if (farReport.findable.length > 1000 && farReport.scroll < 100) {
			break;
		}
		await delay(100);
	}
	const fullText = longDocument.getText();
	assert.ok(
		farReport.scroll < 100,
		`the view is at the top of the document: ${farReport.scroll}`,
	);
	// The rendered text is fetched for the document once it has settled, so
	// the widget can search it however the reader opens it.
	for (let attempt = 0; attempt < 60; attempt += 1) {
		farReport = exports.panelReport();
		if (farReport.findable.length > 1000) {
			break;
		}
		await delay(100);
	}
	assert.ok(
		farReport.findable.length > 1000,
		`the rendered text is ready: ${farReport.findable.length}`,
	);
	assert.strictEqual(
		farReport.ordered,
		true,
		"the drawn spans are in reading order",
	);
	assert.strictEqual(
		farReport.unique,
		true,
		"each source offset is drawn once",
	);

	// Find searches the rendered text, so a phrase in an unvisited section is
	// found and the preview goes to the passage it names.
	const phrase = "第399段";
	assert.ok(
		fullText.includes(phrase),
		"the phrase is in the document",
	);
	const found = await exports.findInPreview(phrase);
	assert.strictEqual(found, undefined, "the search ran");
	await delay(900);
	const foundAt = exports.panelReport();
	say("find-raw", {
		rendered: foundAt.rendered.length,
		findable: foundAt.findable.length,
		scroll: Math.round(foundAt.scroll),
		revealed: foundAt.revealed,
		at: foundAt.findable.head,
	});
	// The rendered text is not the source: markup is not drawn, so it is
	// shorter than the document and holds the words the reader sees.
	assert.ok(
		foundAt.findable.length > 1000 &&
			foundAt.findable.length < fullText.length,
		`the rendered text is in the page: ${foundAt.findable.length} against ${fullText.length} of source`,
	);
	assert.ok(
		foundAt.rendered.indexOf(phrase) >= 0,
		`the rendered text holds the phrase: ${foundAt.rendered.length} characters`,
	);
	assert.ok(
		foundAt.scroll > farReport.scroll + 1000,
		`the preview went to the passage: ${farReport.scroll} then ${foundAt.scroll}`,
	);
	const matchAt = fullText.indexOf(phrase);
	assert.strictEqual(
		foundAt.revealed,
		matchAt,
		`the source range of the match was followed exactly: ${foundAt.revealed} against ${matchAt}`,
	);
	say("find", {
		phrase,
		scroll: [Math.round(farReport.scroll), Math.round(foundAt.scroll)],
		revealed: foundAt.revealed,
	});

	say("input", {
		moved: [fragment.to, movedAnchor[0].to, Math.round(movedScroll)],
		whole: [
			farReport.scroll,
			farReport.findable.length,
			farReport.text.length,
		],
		links: linkMap.map((link) => [
			link.kind,
			link.target,
			link.to,
			Math.round(link.pageY * 10) / 10,
			Math.round(link.x),
		]),
		routed,
		clicked: [cluster.source_start, cluster.source_end, selectedStart, selectedEnd],
		selected: mirrored.text,
	});
	probe.dispose();

	// EXT-8: every setting comes from VS Code configuration, resolved per
	// document, including workspace-folder and language-scoped layers.
	const folders = (vscode.workspace.workspaceFolders ?? []).map(
		(candidate) => candidate.uri.fsPath,
	);
	say("workspace", { folders });
	const firstFolder =
		vscode.workspace.workspaceFolders?.find((candidate) =>
			candidate.uri.fsPath.endsWith("ws"),
		);
	const secondFolder =
		vscode.workspace.workspaceFolders?.find((candidate) =>
			candidate.uri.fsPath.endsWith("ws2"),
		);
	assert.ok(
		firstFolder && secondFolder,
		`two folders are open: ${JSON.stringify(folders)}`,
	);

	/** Opens a document and answers with the settings the panel resolved. */
	const settingsFor = async (file, contents) => {
		fs.writeFileSync(file, contents);
		const document = await vscode.workspace.openTextDocument(file);
		await vscode.commands.executeCommand("markview.closePreview");
		await delay(300);
		await vscode.window.showTextDocument(document, {
			viewColumn: vscode.ViewColumn.One,
			preserveFocus: false,
			preview: false,
		});
		await delay(500);
		await vscode.commands.executeCommand("markview.openPreview", document.uri);
		for (let attempt = 0; attempt < 60; attempt += 1) {
			const report = exports.panelReport();
			if (report.live >= 1 && report.settings.font_size !== undefined) {
				return report.settings;
			}
			await delay(100);
		}
		return exports.panelReport().settings;
	};

	// The first folder's own size, and its language-scoped width.
	const inFirst = await settingsFor(
		path.join(firstFolder.uri.fsPath, "inside.md"),
		"# Inside\n\nA paragraph in a folder.\n",
	);
	assert.strictEqual(
		inFirst.font_size,
		30,
		`the first folder's setting was used: ${JSON.stringify(inFirst)}`,
	);
	// The same file's language-scoped width, which the editor resolves from
	// the document rather than from its URI.
	assert.strictEqual(
		inFirst.width,
		500,
		`the markdown-scoped width was used: ${JSON.stringify(inFirst)}`,
	);

	// The second folder has its own size and no language scope.
	const inSecond = await settingsFor(
		path.join(secondFolder.uri.fsPath, "other.md"),
		"# Other\n\nAnother paragraph.\n",
	);
	assert.strictEqual(
		inSecond.font_size,
		12,
		`the second folder's setting was used: ${JSON.stringify(inSecond)}`,
	);
	assert.strictEqual(
		inSecond.width,
		760,
		`and its own width, not the first folder's: ${JSON.stringify(inSecond)}`,
	);

	// A language-scoped setting applies to the language it names and not to
	// another one, in the same folder.
	const inFirstText = await settingsFor(
		path.join(firstFolder.uri.fsPath, "notes.txt"),
		"Not markdown.\n",
	);
	assert.strictEqual(
		inFirstText.font_size,
		30,
		`the folder's setting applies whatever the language: ${JSON.stringify(inFirstText)}`,
	);
	say("settings", {
		first: inFirst,
		second: inSecond,
		text: inFirstText,
	});

	// A setting that shapes the layout reaches a preview that is already open:
	// the document at hand is laid out again rather than waiting for the next
	// edit or the next open.
	const live = await settingsFor(
		path.join(firstFolder.uri.fsPath, "live.md"),
		`# Live\n\n${"A paragraph that wraps at the column. ".repeat(20)}\n`,
	);
	assert.strictEqual(live.font_size, 30, JSON.stringify(live));
	const beforeChange = exports.panelReport().documentHeight;
	const folder = vscode.workspace.getConfiguration("markview", firstFolder.uri);
	await folder.update("fontSize", 24, vscode.ConfigurationTarget.WorkspaceFolder);
	let reconfigured = exports.panelReport();
	for (let attempt = 0; attempt < 60; attempt += 1) {
		reconfigured = exports.panelReport();
		if (reconfigured.settings.font_size === 24) {
			break;
		}
		await delay(100);
	}
	assert.strictEqual(
		reconfigured.settings.font_size,
		24,
		`the change reached the open preview: ${JSON.stringify(reconfigured.settings)}`,
	);
	assert.notStrictEqual(
		Math.round(reconfigured.documentHeight),
		Math.round(beforeChange),
		`and the document was laid out again: ${beforeChange} then ${reconfigured.documentHeight}`,
	);
	await folder.update(
		"fontSize",
		undefined,
		vscode.ConfigurationTarget.WorkspaceFolder,
	);
	say("settings-live", {
		before: Math.round(beforeChange),
		after: Math.round(reconfigured.documentHeight),
		font_size: reconfigured.settings.font_size,
	});

	// EXT-9: the commands are the editor's own surfaces, and the preview is
	// not one of them: it contributes no keys.
	const contributes = extension.packageJSON.contributes;
	const palette = (contributes.menus.commandPalette ?? []).map(
		(entry) => entry.command,
	);
	for (const id of [
		"markview.openPreview",
		"markview.closePreview",
		"markview.exportPdf",
		"markview.exportPng",
		"markview.exportWithTemplate",
		"markview.revealSource",
	]) {
		assert.ok(palette.includes(id), `${id} is in the command palette`);
	}
	assert.ok(
		(contributes.menus["editor/title"] ?? []).length > 0,
		"the editor title carries a command",
	);
	assert.ok(
		contributes.keybindings === undefined,
		"the preview binds no keys of its own",
	);
	say("commands", { palette: palette.length, keybindings: 0 });

	// EXT-10: the appearance follows the editor's colour theme, and the
	// pixels are what changes.
	const workbench = vscode.workspace.getConfiguration("workbench");
	await workbench.update(
		"colorTheme",
		"Default Dark Modern",
		vscode.ConfigurationTarget.Global,
	);
	await delay(1500);
	const dark = exports.panelReport();
	assert.strictEqual(
		dark.appearance.theme,
		"dark",
		`the engine was told the editor's theme: ${JSON.stringify(dark.appearance)}`,
	);
	// A tile is only fetched for a band that is not already on screen, so the
	// view is taken away and brought back to make the engine draw it again in
	// the appearance it now has.
	const tileAt = async (offset) => {
		const before = exports.panelReport().painted;
		await exports.scrollPreviewTo(offset + 3000);
		await delay(500);
		await exports.scrollPreviewTo(offset);
		for (let attempt = 0; attempt < 30; attempt += 1) {
			await delay(100);
			const report = exports.panelReport();
			if (report.painted !== before && report.painted > 0) {
				return report.painted;
			}
		}
		return exports.panelReport().painted;
	};
	const darkTile = await tileAt(80);
	await workbench.update(
		"colorTheme",
		"Default Light Modern",
		vscode.ConfigurationTarget.Global,
	);
	await delay(1500);
	const light = exports.panelReport();
	assert.strictEqual(
		light.appearance.theme,
		"light",
		`and follows it back: ${JSON.stringify(light.appearance)}`,
	);
	const lightTile = await tileAt(80);
	assert.notStrictEqual(
		lightTile,
		darkTile,
		`the pixels changed with the theme: ${darkTile} then ${lightTile}`,
	);
	say("appearance", {
		themes: [dark.appearance.theme, light.appearance.theme],
		painted: [darkTile, lightTile],
		reflow: [dark.appearance.reflow, light.appearance.reflow],
	});

	// EXT-12: the export command runs and the result is revealed. A document
	// that is not Markdown is refused rather than exported as something else;
	// which editor holds the focus is the test host's business rather than the
	// command's, so the document is named instead of brought to the front.
	const plainPath = path.join(firstFolder.uri.fsPath, "notes.txt");
	await vscode.workspace.openTextDocument(plainPath);
	const refused = path.join(scratch, "refused.pdf");
	await assert.rejects(exports.exportDocument({
		uri: vscode.Uri.file(plainPath),
		target: vscode.Uri.file(refused),
		format: "pdf",
	}));
	await delay(400);
	assert.ok(
		!fs.existsSync(refused),
		"a document that is not Markdown was not exported",
	);

	// The export writes the document the editor is showing.
	await vscode.window.showTextDocument(longDocument, {
		viewColumn: vscode.ViewColumn.One,
		preserveFocus: false,
		preview: false,
	});
	await delay(600);
	const exported = path.join(scratch, "exported.pdf");
	await vscode.commands.executeCommand(
		"markview.exportPdf",
		{ target: vscode.Uri.file(exported) },
	);
	for (let attempt = 0; attempt < 100; attempt += 1) {
		if (fs.existsSync(exported)) {
			break;
		}
		await delay(100);
	}
	assert.ok(fs.existsSync(exported), "the export wrote a file");
	assert.ok(
		fs.statSync(exported).size > 1000,
		`and it has a page in it: ${fs.statSync(exported).size} bytes`,
	);
	assert.strictEqual(
		exports.panelReport().exported,
		exported,
		"the panel recorded what it exported",
	);
	say("export", {
		bytes: fs.statSync(exported).size,
		path: path.basename(exported),
	});

	// The same document as one PNG, and as a PDF under a template the caller
	// holds rather than installs: both are what "render by template" means.
	const png = path.join(scratch, "exported.png");
	await exports.exportDocument({ uri: longDocument.uri, target: vscode.Uri.file(png), format: "png" });
	assert.ok(fs.existsSync(png), "the PNG export wrote a file");
	const pngBytes = fs.readFileSync(png);
	assert.deepStrictEqual(
		[...pngBytes.subarray(0, 8)],
		[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a],
		"and it is a PNG",
	);

	const template = path.join(scratch, "house.mvss.toml");
	fs.writeFileSync(
		template,
		[
			'format_version = 2',
			'targets = ["pdf"]',
			"version = 1",
			"[[rule]]",
			'when = ["body"]',
			'color = "#0B3D2E"',
			"line_height = 1.9",
			"",
		].join("\n"),
	);
	const templated = path.join(scratch, "templated.pdf");
	await exports.exportDocument({
		uri: longDocument.uri,
		target: vscode.Uri.file(templated),
		format: "pdf",
		template: path.relative(path.dirname(longDocument.uri.fsPath), template),
	});
	assert.ok(fs.existsSync(templated), "the templated export wrote a file");
	assert.notDeepStrictEqual(
		fs.readFileSync(templated),
		fs.readFileSync(exported),
		"the template changed what was written",
	);
	const templatedPng = path.join(scratch, "templated.png");
	await exports.exportDocument({
		uri: longDocument.uri,
		target: vscode.Uri.file(templatedPng),
		format: "png",
		template: path.relative(path.dirname(longDocument.uri.fsPath), template),
	});
	assert.notDeepStrictEqual(
		fs.readFileSync(templatedPng),
		pngBytes,
		"the template changed the image too",
	);

	// The setting is resolved for the document at hand, and asking for no
	// template means no template rather than falling back to the setting.
	await vscode.workspace
		.getConfiguration("markview")
		.update("template", path.relative(path.dirname(longDocument.uri.fsPath), template), vscode.ConfigurationTarget.Global);
	await delay(200);
	const fromSetting = path.join(scratch, "from-setting.pdf");
	await exports.exportDocument({
		target: vscode.Uri.file(fromSetting),
		format: "pdf",
	});
	assert.ok(fs.existsSync(fromSetting), "the setting's template was used");
	assert.notDeepStrictEqual(
		fs.readFileSync(fromSetting),
		fs.readFileSync(exported),
		"and it is the template the setting names",
	);
	const none = path.join(scratch, "none.pdf");
	await exports.exportDocument({
		target: vscode.Uri.file(none),
		format: "pdf",
		template: null,
	});
	assert.deepStrictEqual(
		fs.readFileSync(none),
		fs.readFileSync(exported),
		"asking for no template wrote the bundled sheet, not the setting's",
	);
	await vscode.workspace
		.getConfiguration("markview")
		.update("template", undefined, vscode.ConfigurationTarget.Global);
	await delay(200);

	// A template nobody has is refused rather than quietly ignored.
	const unknown = path.join(scratch, "unknown.pdf");
	await assert.rejects(exports.exportDocument({
		target: vscode.Uri.file(unknown),
		format: "pdf",
		template: "no-such-template",
	}));
	assert.ok(
		!fs.existsSync(unknown),
		"an unknown template wrote nothing rather than falling back",
	);

	// The templates the panel offers are the engine's own list, not a copy of
	// it that could drift.
	const templates = await exports.templates();
	const ids = templates.map((entry) => entry.id);
	for (const id of ["print", "mondrian"]) {
		assert.ok(ids.includes(id), `${id} is offered: ${ids.join(", ")}`);
	}
	assert.ok(
		templates.every((entry) => !entry.error),
		"every template the engine lists reads: " +
			JSON.stringify(templates.filter((entry) => entry.error)),
	);
	say("render", {
		png: pngBytes.length,
		templated: fs.statSync(templated).size,
		templatedPng: fs.statSync(templatedPng).size,
		templates: ids.length,
	});

	say("document-height", Math.round(observed.documentHeight));
	say("scroll-extent", Math.round(observed.scrollHeight));
	say("viewport", observed.viewport);
	say("screens", screens);
	say("bands-fetched", fetched);
	say("bands-live", after.live);
	await require("./regressions.js").run(extension, scratch);
	await require("./latency.js").run(exports, scratch);
	fs.rmSync(scratch, { recursive: true, force: true });
	say("ok", true);
};
