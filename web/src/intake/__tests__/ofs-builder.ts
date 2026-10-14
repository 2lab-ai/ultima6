import { ADF_SIZE } from "../mfm.js";

// Test-only AmigaDOS image builder (OFS or FFS), enough structure for the reader:
// boot signature, root block 880, hash tables with chains, file headers with
// extension blocks, OFS data block headers, block checksums.

const BS = 512;

export type Node = { name: string; data: Uint8Array } | { name: string; children: Node[] };

export function amigaHash(name: string): number {
  let h = name.length;
  for (const ch of name.toUpperCase()) h = (h * 13 + ch.charCodeAt(0)) & 0x7ff;
  return h % 72;
}

export class ImageBuilder {
  img = new Uint8Array(ADF_SIZE);
  private next = 882;

  constructor(private ffs: boolean) {
    this.img.set([0x44, 0x4f, 0x53, ffs ? 1 : 0], 0);
  }

  w32(block: number, off: number, v: number): void {
    new DataView(this.img.buffer).setUint32(block * BS + off, v >>> 0);
  }

  r32(block: number, off: number): number {
    return new DataView(this.img.buffer).getUint32(block * BS + off);
  }

  name(block: number, s: string): void {
    this.img[block * BS + 432] = s.length;
    this.img.set(new TextEncoder().encode(s), block * BS + 433);
  }

  checksum(block: number): void {
    this.w32(block, 20, 0);
    let sum = 0;
    for (let i = 0; i < BS; i += 4) sum = (sum + this.r32(block, i)) >>> 0;
    this.w32(block, 20, -sum);
  }

  alloc(): number {
    return this.next++;
  }

  /** Link `entry` into `dir`'s hash table, chaining on collision. */
  link(dir: number, entry: number, name: string): void {
    const slot = 24 + amigaHash(name) * 4;
    let cur = this.r32(dir, slot);
    if (cur === 0) {
      this.w32(dir, slot, entry);
      return;
    }
    while (this.r32(cur, BS - 16) !== 0) cur = this.r32(cur, BS - 16);
    this.w32(cur, BS - 16, entry);
  }

  file(parent: number, name: string, data: Uint8Array): number {
    const hdr = this.alloc();
    const per = this.ffs ? BS : BS - 24;
    const nblocks = Math.ceil(data.length / per);
    const dataBlocks = Array.from({ length: nblocks }, () => this.alloc());
    dataBlocks.forEach((db, i) => {
      const chunk = data.subarray(i * per, (i + 1) * per);
      if (this.ffs) {
        this.img.set(chunk, db * BS);
      } else {
        this.w32(db, 0, 8);
        this.w32(db, 4, hdr);
        this.w32(db, 8, i + 1);
        this.w32(db, 12, chunk.length);
        this.w32(db, 16, dataBlocks[i + 1] ?? 0);
        this.img.set(chunk, db * BS + 24);
        this.checksum(db);
      }
    });
    // Header + extension blocks, 72 pointers each, stored from offset 308 downwards.
    let table = hdr;
    for (let start = 0; start < Math.max(nblocks, 1); start += 72) {
      const part = dataBlocks.slice(start, start + 72);
      part.forEach((db, i) => this.w32(table, 308 - i * 4, db));
      this.w32(table, 8, part.length);
      this.w32(table, 16, part[0] ?? 0);
      this.w32(table, BS - 4, -3);
      this.w32(table, BS - 12, table === hdr ? parent : hdr);
      const ext = start + 72 < nblocks ? this.alloc() : 0;
      this.w32(table, BS - 8, ext);
      if (table === hdr) {
        this.w32(hdr, 0, 2);
        this.w32(hdr, 4, hdr);
        this.w32(hdr, 324, data.length);
        this.name(hdr, name);
      } else {
        this.w32(table, 0, 16);
        this.w32(table, 4, table);
      }
      this.checksum(table);
      table = ext;
    }
    this.link(parent, hdr, name);
    return hdr;
  }

  dir(parent: number, name: string, children: Node[]): number {
    const b = this.alloc();
    this.w32(b, 0, 2);
    this.w32(b, 4, b);
    this.w32(b, BS - 4, 2);
    this.w32(b, BS - 12, parent);
    this.name(b, name);
    this.add(b, children);
    this.checksum(b);
    this.link(parent, b, name);
    return b;
  }

  add(dir: number, nodes: Node[]): void {
    for (const n of nodes) {
      if ("data" in n) this.file(dir, n.name, n.data);
      else this.dir(dir, n.name, n.children);
    }
  }

  build(volume: string, nodes: Node[]): Uint8Array {
    const root = 880;
    this.w32(root, 0, 2);
    this.w32(root, 12, 72);
    this.w32(root, BS - 4, 1);
    this.name(root, volume);
    this.add(root, nodes);
    // Re-checksum every header touched by hash chaining.
    for (let b = 880; b < this.next; b++) {
      if (b !== 881 && this.r32(b, 0) === 2) this.checksum(b);
    }
    return this.img;
  }
}
