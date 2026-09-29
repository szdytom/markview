const { test } = require('node:test');
const assert = require('node:assert/strict');
const { Frames } = require('../../vscode/out/shared/frames.js');
const png = Buffer.from([137, 80, 78, 71, 13, 10, 26, 10, 0, 255, 123, 125]);
const line = value => Buffer.from(JSON.stringify(value) + '\n');
const header = line({ tile: { id: '文档', encoding: 'png', bytes: png.length } });
const wire = Buffer.concat([line({ opened: { id: '文档' } }), header, png, line({ layout: { id: '文档' } }), line({ saved: {} }), header, png]);
test('fragmented and coalesced pipe data preserves PNGs, UTF-8 and notification order', () => {
    for (let size = 1; size <= wire.length; size++) {
        const answers = [];
        const frames = new Frames(answer => answers.push(answer));
        for (let offset = 0; offset < wire.length; offset += size) frames.push(wire.subarray(offset, offset + size));
        frames.finish();
        assert.deepEqual(answers.map(answer => Object.keys(answer)[0]), ['opened', 'tile', 'layout', 'saved', 'tile']);
        assert.equal(answers[1].tile.id, '文档');
        for (const index of [1, 4]) {
            const pixels = answers[index].tile.png;
            assert.equal(pixels.constructor, Uint8Array);
            assert.deepEqual(Buffer.from(pixels), png);
            assert.equal(pixels.buffer.byteLength, png.length, 'no pooled unrelated bytes in the payload');
        }
    }
});
test('a tile is not delivered until its whole binary body arrives', () => {
    const answers = [];
    const frames = new Frames(answer => answers.push(answer));
    frames.push(Buffer.concat([header, png.subarray(0, -1)]));
    assert.equal(answers.length, 0);
    assert.throws(() => frames.finish(), /Truncated/);
    frames.push(png.subarray(-1));
    frames.finish();
    assert.equal(answers.length, 1);
});
test('invalid lengths, legacy base64 and truncated headers are refused', () => {
    for (const bytes of [-1, 0, 1.5, '12', 64 * 1024 * 1024 + 1]) {
        assert.throws(() => new Frames(() => {}).push(line({ tile: { encoding: 'png', bytes } })), /length/);
    }
    assert.throws(() => new Frames(() => {}).push(line({ tile: { png: 'legacy' } })), /binary PNG/);
    assert.throws(() => new Frames(() => {}).push(Buffer.from('bad\n')), SyntaxError);
    const frames = new Frames(() => {});
    frames.push(Buffer.from('{"saved":'));
    assert.throws(() => frames.finish(), /Truncated/);
});
