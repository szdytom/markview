// Read the executable header instead of trusting a filename or the build host.
const fs = require("node:fs");
const [binary, expected] = process.argv.slice(2);
const fd = fs.openSync(binary, "r");
try {
	const header = Buffer.alloc(64);
	fs.readSync(fd, header, 0, 64, 0);
	let platform;
	if (header.readUInt32LE(0) === 0xfeedfacf) {
		platform = { 0x01000007: "darwin-x64", 0x0100000c: "darwin-arm64" }[header.readUInt32LE(4)];
	} else if (header.subarray(0, 4).equals(Buffer.from([0x7f, 0x45, 0x4c, 0x46])) && header[4] === 2 && header[5] === 1) {
		platform = { 62: "linux-x64", 183: "linux-arm64" }[header.readUInt16LE(18)];
	} else if (header.toString("ascii", 0, 2) === "MZ") {
		const pe = Buffer.alloc(6);
		fs.readSync(fd, pe, 0, 6, header.readUInt32LE(60));
		if (pe.readUInt32LE(0) === 0x4550) platform = { 0x8664: "win32-x64", 0xaa64: "win32-arm64" }[pe.readUInt16LE(4)];
	}
	if (platform !== expected) throw new Error(`Expected ${expected}, found ${platform ?? "unknown executable"}: ${binary}`);
	console.log(`${platform}: ${binary}`);
} finally {
	fs.closeSync(fd);
}
