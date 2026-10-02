import assert from "node:assert/strict";
import { test } from "node:test";
import { ScrollSync, sourceToAnchor, anchorToSource } from "../dist/index.js";

test("a wrapped image follows its entire source extent in both directions", () => {
	const source = { start: 5, end: 900 };
	const measure = (range) => {
		assert.deepEqual(range, source);
		return { top: 10, bottom: 30 };
	};
	const anchors = [0, 1, 2, 3, 4].map((i) =>
		sourceToAnchor(
			{ offset: 5 + i * 200, top: 10 + i * 5 },
			source,
			measure,
		),
	);
	assert.deepEqual(
		anchors.map((a) => a.fraction),
		[0, 0.25, 0.5, 0.75, 1],
	);
	assert.deepEqual(
		anchors.map((a) => anchorToSource(a, source, measure)),
		[10, 15, 20, 25, 30],
	);
	// Hosts may use fractional line coordinates instead of CSS pixels.
	const anchor = sourceToAnchor({ offset: 5, top: 0.15 }, source, () => ({
		top: 0.1,
		bottom: 0.2,
	}));
	assert.ok(Math.abs(anchor.fraction - 0.5) < 1e-12);
});

test("source gaps clamp progress while missing geometry retains the reading offset", () => {
	const source = { start: 5, end: 900 };
	const measure = () => ({ top: 10, bottom: 30 });
	assert.equal(
		sourceToAnchor({ offset: 0, top: 5 }, source, measure).fraction,
		0,
	);
	assert.equal(
		sourceToAnchor({ offset: 902, top: 35 }, source, measure).fraction,
		1,
	);
	assert.equal(
		anchorToSource({ offset: 5, fraction: -0.25 }, source, measure),
		5,
	);
	assert.equal(
		anchorToSource({ offset: 5, fraction: 1.25 }, source, measure),
		35,
	);
	const anchor = sourceToAnchor({ offset: 87, top: 20 }, null, (range) => {
		assert.deepEqual(range, { start: 87, end: 87 });
		return null;
	});
	assert.deepEqual(anchor, { offset: 87, fraction: 0 });
	assert.deepEqual(sourceToAnchor({ offset: 87, top: 15 }, null, measure), {
		offset: 87,
		fraction: 0.25,
	});
	assert.equal(
		anchorToSource(anchor, null, () => null),
		null,
	);
	assert.equal(anchorToSource(anchor, source, measure), 10);
});

test("host messages reject feedback, older measurements, switched input and replaced documents", () => {
	const sync = new ScrollSync(1);
	assert.equal(sync.owner, "source");
	assert.equal(sync.begin("preview"), null);
	const old = sync.begin("source");
	const latest = sync.begin("source");
	assert.equal(sync.isCurrent(old), false);
	assert.equal(sync.isCurrent(structuredClone(latest)), true);
	assert.equal(sync.begin("preview"), null);
	assert.equal(sync.isCurrent(latest), true);

	sync.takeControl("preview");
	assert.equal(sync.isCurrent(latest), false);
	const preview = sync.begin("preview", 1);
	assert.equal(sync.begin("source"), null);
	assert.equal(sync.isCurrent(preview), true);
	sync.takeControl("preview");
	assert.equal(sync.isCurrent(preview), false);

	const beforeEdit = sync.begin("preview", 1);
	sync.setDocumentVersion(2);
	assert.equal(sync.isCurrent(beforeEdit), false);
	assert.equal(sync.begin("preview", 1), null);
	const afterEdit = sync.begin("preview", 2);
	sync.setDocumentVersion(2);
	assert.equal(sync.isCurrent(afterEdit), true);
	sync.cancel();
	assert.equal(sync.isCurrent(afterEdit), false);
});
