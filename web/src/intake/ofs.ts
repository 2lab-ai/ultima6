// AmigaDOS filesystem reader (OFS "DOS\0", plus FFS "DOS\1" data blocks) for
// 880 KB double-density images. Block = 512 bytes, big-endian longs.
//
// Header blocks (root, user dir, file header, file extension) share a layout:
// type @0, header_key @4, high_seq @8, table size @12, first_data @16,
// checksum @20, hash/data-block table @24..311, byte_size @324, name (BCPL) @432,
// hash_chain @496, parent @500, extension @504, sec_type @508.

const BS = 512;
const ROOT_BLOCK_DD = 880;
const TABLE_AT = 24;
const TABLE_LONGS = (BS - 200 - TABLE_AT) / 4; // 72 on 512-byte blocks

const T_HEADER = 2;
const T_DATA = 8;
const T_LIST = 16;
const ST_ROOT = 1;
const ST_USERDIR = 2;
const ST_FILE = -3;

export interface OfsVolume {
  name: string;
  dosType: string;
  /** "dir/sub/file" (relative to the volume) → file bytes, in directory walk order. */
  files: Map<string, Uint8Array>;
  /** Directory paths seen (relative to the volume, "" omitted). */
  dirs: string[];
  /** Problems found while walking (bad checksums, broken chains). */
  warnings: string[];
}

function u32(img: Uint8Array, at: number): number {
  return (
    (((img[at] ?? 0) << 24) | ((img[at + 1] ?? 0) << 16) | ((img[at + 2] ?? 0) << 8) | (img[at + 3] ?? 0)) >>> 0
  );
}

function s32(img: Uint8Array, at: number): number {
  return u32(img, at) | 0;
}

function blockChecksumOk(img: Uint8Array, block: number): boolean {
  let sum = 0;
  const base = block * BS;
  for (let i = 0; i < BS; i += 4) sum = (sum + u32(img, base + i)) >>> 0;
  return sum === 0;
}

function bcplName(img: Uint8Array, block: number): string {
  const base = block * BS + 432;
  const len = Math.min(img[base] ?? 0, 30);
  let s = "";
  for (let i = 0; i < len; i++) s += String.fromCharCode(img[base + 1 + i] ?? 0);
  return s;
}

export function adfDosType(img: Uint8Array): string | null {
  if (img.length < 4 || img[0] !== 0x44 || img[1] !== 0x4f || img[2] !== 0x53) return null;
  return `DOS${img[3] ?? 0}`;
}

export function readOfs(img: Uint8Array): OfsVolume {
  const dosType = adfDosType(img);
  if (!dosType) throw new Error("not an AmigaDOS image (boot block lacks DOS signature)");
  const flags = img[3] ?? 0;
  if (flags > 7) throw new Error(`unsupported AmigaDOS type ${dosType}`);
  const ffs = (flags & 1) === 1;
  const blocks = Math.floor(img.length / BS);
  const root = ROOT_BLOCK_DD;
  if (root >= blocks) throw new Error("image too small for a double-density root block");
  if (s32(img, root * BS) !== T_HEADER || s32(img, root * BS + BS - 4) !== ST_ROOT) {
    throw new Error(`block ${root} is not a root block`);
  }

  const vol: OfsVolume = { name: bcplName(img, root), dosType, files: new Map(), dirs: [], warnings: [] };
  const seen = new Set<number>();
  const valid = (b: number): boolean => b > 1 && b < blocks;

  const checkHeader = (b: number, what: string): void => {
    if (!blockChecksumOk(img, b)) vol.warnings.push(`${what}: bad block checksum at ${b}`);
  };

  const readFile = (hdr: number, path: string): Uint8Array => {
    const size = u32(img, hdr * BS + 324);
    const out = new Uint8Array(size);
    const perBlock = ffs ? BS : BS - 24;
    let written = 0;
    let table = hdr;
    let hops = 0;
    while (table !== 0 && written < size) {
      if (!valid(table) || hops++ > blocks) throw new Error(`${path}: broken extension chain`);
      if (table !== hdr) {
        if (s32(img, table * BS) !== T_LIST) throw new Error(`${path}: block ${table} is not an extension block`);
        checkHeader(table, path);
      }
      const count = Math.min(u32(img, table * BS + 8), TABLE_LONGS);
      for (let i = 0; i < count && written < size; i++) {
        const db = u32(img, table * BS + TABLE_AT + (TABLE_LONGS - 1 - i) * 4);
        if (!valid(db)) throw new Error(`${path}: bad data block pointer ${db}`);
        let from = db * BS;
        let n = Math.min(perBlock, size - written);
        if (!ffs) {
          if (s32(img, from) !== T_DATA || u32(img, from + 4) !== hdr) {
            throw new Error(`${path}: block ${db} is not a data block of this file`);
          }
          if (!blockChecksumOk(img, db)) vol.warnings.push(`${path}: bad data block checksum at ${db}`);
          n = Math.min(u32(img, from + 12), n);
          from += 24;
        }
        out.set(img.subarray(from, from + n), written);
        written += n;
      }
      table = u32(img, table * BS + BS - 8);
    }
    if (written !== size) throw new Error(`${path}: short read ${written}/${size}`);
    return out;
  };

  const walkDir = (dirBlock: number, prefix: string): void => {
    for (let i = 0; i < TABLE_LONGS; i++) {
      let b = u32(img, dirBlock * BS + TABLE_AT + i * 4);
      while (b !== 0) {
        if (!valid(b)) {
          vol.warnings.push(`${prefix || "/"}: bad hash entry ${b}`);
          break;
        }
        if (seen.has(b)) {
          vol.warnings.push(`${prefix || "/"}: loop at block ${b}`);
          break;
        }
        seen.add(b);
        const name = bcplName(img, b);
        const path = prefix ? `${prefix}/${name}` : name;
        const type = s32(img, b * BS);
        const sec = s32(img, b * BS + BS - 4);
        if (type !== T_HEADER) {
          vol.warnings.push(`${path}: block ${b} has type ${type}`);
        } else {
          checkHeader(b, path);
          if (sec === ST_FILE) {
            try {
              vol.files.set(path, readFile(b, path));
            } catch (err) {
              vol.warnings.push(err instanceof Error ? err.message : String(err));
            }
          } else if (sec === ST_USERDIR) {
            vol.dirs.push(path);
            walkDir(b, path);
          } else {
            vol.warnings.push(`${path}: skipped entry with sec_type ${sec}`);
          }
        }
        b = u32(img, b * BS + BS - 16);
      }
    }
  };

  checkHeader(root, "root");
  walkDir(root, "");
  return vol;
}
