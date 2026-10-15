//! Test-only AmigaDOS image builder (OFS or FFS), enough structure for the reader:
//! boot signature, root block 880, hash tables with chains, file headers with
//! extension blocks, OFS data block headers, block checksums.

use u6_intake::ADF_SIZE;

const BS: usize = 512;

pub enum Node {
    File(&'static str, Vec<u8>),
    Dir(&'static str, Vec<Node>),
}

pub fn amiga_hash(name: &str) -> usize {
    let mut h = name.len() as u32;
    for ch in name.to_uppercase().chars() {
        h = (h * 13 + ch as u32) & 0x7ff;
    }
    (h % 72) as usize
}

pub struct ImageBuilder {
    pub img: Vec<u8>,
    next: usize,
    ffs: bool,
}

impl ImageBuilder {
    pub fn new(ffs: bool) -> Self {
        let mut img = vec![0u8; ADF_SIZE];
        img[..4].copy_from_slice(&[0x44, 0x4f, 0x53, u8::from(ffs)]);
        ImageBuilder {
            img,
            next: 882,
            ffs,
        }
    }

    pub fn w32(&mut self, block: usize, off: usize, v: u32) {
        let at = block * BS + off;
        self.img[at..at + 4].copy_from_slice(&v.to_be_bytes());
    }

    pub fn r32(&self, block: usize, off: usize) -> u32 {
        let at = block * BS + off;
        u32::from_be_bytes(self.img[at..at + 4].try_into().unwrap())
    }

    fn name(&mut self, block: usize, s: &str) {
        let at = block * BS + 432;
        self.img[at] = s.len() as u8;
        self.img[at + 1..at + 1 + s.len()].copy_from_slice(s.as_bytes());
    }

    fn checksum(&mut self, block: usize) {
        self.w32(block, 20, 0);
        let sum = (0..BS)
            .step_by(4)
            .fold(0u32, |s, i| s.wrapping_add(self.r32(block, i)));
        self.w32(block, 20, sum.wrapping_neg());
    }

    fn alloc(&mut self) -> usize {
        self.next += 1;
        self.next - 1
    }

    /// Link `entry` into `dir`'s hash table, chaining on collision.
    fn link(&mut self, dir: usize, entry: usize, name: &str) {
        let slot = 24 + amiga_hash(name) * 4;
        let mut cur = self.r32(dir, slot) as usize;
        if cur == 0 {
            self.w32(dir, slot, entry as u32);
            return;
        }
        while self.r32(cur, BS - 16) != 0 {
            cur = self.r32(cur, BS - 16) as usize;
        }
        self.w32(cur, BS - 16, entry as u32);
    }

    fn file(&mut self, parent: usize, name: &str, data: &[u8]) -> usize {
        let hdr = self.alloc();
        let per = if self.ffs { BS } else { BS - 24 };
        let nblocks = data.len().div_ceil(per);
        let data_blocks: Vec<usize> = (0..nblocks).map(|_| self.alloc()).collect();
        for (i, (&db, chunk)) in data_blocks.iter().zip(data.chunks(per)).enumerate() {
            if self.ffs {
                self.img[db * BS..db * BS + chunk.len()].copy_from_slice(chunk);
            } else {
                self.w32(db, 0, 8);
                self.w32(db, 4, hdr as u32);
                self.w32(db, 8, i as u32 + 1);
                self.w32(db, 12, chunk.len() as u32);
                self.w32(db, 16, data_blocks.get(i + 1).copied().unwrap_or(0) as u32);
                self.img[db * BS + 24..db * BS + 24 + chunk.len()].copy_from_slice(chunk);
                self.checksum(db);
            }
        }
        // Header + extension blocks, 72 pointers each, stored from offset 308 downwards.
        let mut table = hdr;
        for start in (0..nblocks.max(1)).step_by(72) {
            let part = &data_blocks[start..(start + 72).min(nblocks)];
            for (i, &db) in part.iter().enumerate() {
                self.w32(table, 308 - i * 4, db as u32);
            }
            self.w32(table, 8, part.len() as u32);
            self.w32(table, 16, part.first().copied().unwrap_or(0) as u32);
            self.w32(table, BS - 4, (-3i32) as u32);
            self.w32(
                table,
                BS - 12,
                if table == hdr { parent } else { hdr } as u32,
            );
            let ext = if start + 72 < nblocks {
                self.alloc()
            } else {
                0
            };
            self.w32(table, BS - 8, ext as u32);
            if table == hdr {
                self.w32(hdr, 0, 2);
                self.w32(hdr, 4, hdr as u32);
                self.w32(hdr, 324, data.len() as u32);
                self.name(hdr, name);
            } else {
                self.w32(table, 0, 16);
                self.w32(table, 4, table as u32);
            }
            self.checksum(table);
            table = ext;
        }
        self.link(parent, hdr, name);
        hdr
    }

    fn dir(&mut self, parent: usize, name: &str, children: &[Node]) -> usize {
        let b = self.alloc();
        self.w32(b, 0, 2);
        self.w32(b, 4, b as u32);
        self.w32(b, BS - 4, 2);
        self.w32(b, BS - 12, parent as u32);
        self.name(b, name);
        self.add(b, children);
        self.checksum(b);
        self.link(parent, b, name);
        b
    }

    fn add(&mut self, dir: usize, nodes: &[Node]) {
        for n in nodes {
            match n {
                Node::File(name, data) => {
                    self.file(dir, name, data);
                }
                Node::Dir(name, children) => {
                    self.dir(dir, name, children);
                }
            }
        }
    }

    pub fn build(mut self, volume: &str, nodes: &[Node]) -> Vec<u8> {
        let root = 880;
        self.w32(root, 0, 2);
        self.w32(root, 12, 72);
        self.w32(root, BS - 4, 1);
        self.name(root, volume);
        self.add(root, nodes);
        // Re-checksum every header touched by hash chaining.
        for b in 880..self.next {
            if b != 881 && self.r32(b, 0) == 2 {
                self.checksum(b);
            }
        }
        self.img
    }
}

/// Writes the big-endian long `v` at `off` in `block` of a raw image.
pub fn put32(img: &mut [u8], block: usize, off: usize, v: u32) {
    img[block * BS + off..block * BS + off + 4].copy_from_slice(&v.to_be_bytes());
}

/// Sets the checksum long (offset 20) so the block's longs sum to zero.
pub fn checksum_block(img: &mut [u8], block: usize) {
    put32(img, block, 20, 0);
    let sum = (0..BS).step_by(4).fold(0u32, |s, i| {
        let at = block * BS + i;
        s.wrapping_add(u32::from_be_bytes(img[at..at + 4].try_into().unwrap()))
    });
    put32(img, block, 20, sum.wrapping_neg());
}

fn put_name(img: &mut [u8], block: usize, name: &str) {
    img[block * BS + 432] = name.len() as u8;
    img[block * BS + 433..block * BS + 433 + name.len()].copy_from_slice(name.as_bytes());
}

/// An FFS volume (FFS data blocks name no owner) whose `count` file headers, chained from the
/// root, all point at the one data block 883 and are their own extension blocks, so each reads
/// `size` bytes out of a single block of the image.
pub fn shared_block_volume(count: usize, size: u32) -> Vec<u8> {
    // The builder lays out root 880, the header of "a" at 882 and its data block at 883.
    let mut img = ImageBuilder::new(true).build("SHARED", &[Node::File("a", vec![0x33; BS])]);
    let mut prev = 882;
    for k in 0..count {
        let hdr = 884 + k;
        put32(&mut img, hdr, 0, 2); // header
        put32(&mut img, hdr, 4, hdr as u32);
        put32(&mut img, hdr, 8, 72); // a full table of data block pointers
        for i in 0..72 {
            put32(&mut img, hdr, 24 + i * 4, 883);
        }
        put32(&mut img, hdr, 324, size);
        put_name(&mut img, hdr, &format!("s{k:03}"));
        put32(&mut img, hdr, BS - 8, hdr as u32); // the extension: itself
        put32(&mut img, hdr, BS - 4, (-3i32) as u32); // a file
        checksum_block(&mut img, hdr);
        put32(&mut img, prev, BS - 16, hdr as u32); // the hash chain
        checksum_block(&mut img, prev);
        prev = hdr;
    }
    img
}

/// An OFS volume that is one chain of `depth` directories, each the only entry of the one above
/// (hash slot 0), with 30-character names. With `loops`, each directory's other 71 hash slots
/// name the directory itself, a block the walk has already seen. Uses every block but the boot
/// blocks and the root, so at most 1,757 levels.
pub fn nested_dirs(volume: &str, depth: usize, loops: bool) -> Vec<u8> {
    let mut img = ImageBuilder::new(false).build(volume, &[]);
    let blocks: Vec<usize> = (2..ADF_SIZE / BS)
        .filter(|&b| b != 880)
        .take(depth)
        .collect();
    assert_eq!(blocks.len(), depth, "at most 1,757 levels fit");
    let mut parent = 880;
    for (k, &b) in blocks.iter().enumerate() {
        put32(&mut img, b, 0, 2); // header
        put32(&mut img, b, 4, b as u32);
        put32(&mut img, b, BS - 12, parent as u32);
        put32(&mut img, b, BS - 4, 2); // a directory
        put_name(&mut img, b, &format!("level{k:025}"));
        if loops {
            for slot in 1..72 {
                put32(&mut img, b, 24 + slot * 4, b as u32);
            }
        }
        put32(&mut img, parent, 24, b as u32); // slot 0 of the one above
        checksum_block(&mut img, parent);
        parent = b;
    }
    checksum_block(&mut img, parent);
    img
}
