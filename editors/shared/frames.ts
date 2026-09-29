/** JSON lines, with a raw PNG body immediately after a tile header. */
export class Frames {
    private parts: Buffer[] = [];
    private headerBytes = 0;
    private header: Record<string, unknown> | undefined;
    private pixels: Uint8Array | undefined;
    private received = 0;

    constructor(private readonly accept: (message: Record<string, unknown>) => void) {}

    push(chunk: Buffer): void {
        let offset = 0;
        while (offset < chunk.length) {
            if (this.pixels) {
                const count = Math.min(chunk.length - offset, this.pixels.length - this.received);
                this.pixels.set(chunk.subarray(offset, offset + count), this.received);
                offset += count;
                this.received += count;
                if (this.received === this.pixels.length) {
                    const header = this.header!;
                    (header.tile as Record<string, unknown>).png = this.pixels;
                    this.header = undefined;
                    this.pixels = undefined;
                    this.received = 0;
                    this.accept(header);
                }
                continue;
            }
            const newline = chunk.indexOf(10, offset);
            const end = newline < 0 ? chunk.length : newline;
            this.headerBytes += end - offset;
            if (this.headerBytes > 64 * 1024 * 1024) throw new Error('Engine JSON header exceeds 64 MiB');
            this.parts.push(chunk.subarray(offset, end));
            offset = end + (newline < 0 ? 0 : 1);
            if (newline < 0) continue;
            const header = JSON.parse(Buffer.concat(this.parts, this.headerBytes).toString('utf8'));
            this.parts = [];
            this.headerBytes = 0;
            if (!header || typeof header !== 'object' || Array.isArray(header)) throw new Error('Invalid engine response');
            if (header.tile) {
                if (header.tile.encoding !== 'png' || 'png' in header.tile) {
                    throw new Error('The preview needs a newer engine. Update markview.enginePath or use the bundled engine (binary PNG transport).');
                }
                const bytes = header.tile.bytes;
                if (!Number.isSafeInteger(bytes) || bytes < 1 || bytes > 64 * 1024 * 1024) {
                    throw new Error('Invalid engine PNG length');
                }
                this.header = header;
                this.pixels = new Uint8Array(bytes);
            } else {
                this.accept(header);
            }
        }
    }

    finish(): void {
        if (this.headerBytes || this.pixels) throw new Error('Truncated engine response');
    }
}
