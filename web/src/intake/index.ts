// Browser intake: whatever the user drops → Ultima VI Amiga disk images and files.
//
//   zip (archive.org ultima-vi.zip) → *.hfe / *.adf entries
//   HFE → Amiga MFM → ADF (901,120 bytes)
//   ADF → AmigaDOS OFS → "VOLUME/path" files
//   any file starting with "PP20" → PowerPacker decrunch
//
// Loose files (already extracted) pass through with PP20 decrunch. No network,
// no Node APIs: pure functions over Uint8Array.

import { unzipSync } from "fflate";
import { isHfe, parseHfe } from "./hfe.js";
import { ADF_SIZE, decodeAmigaDisk } from "./mfm.js";
import { adfDosType, readOfs } from "./ofs.js";
import { decrunchPP20, isPP20 } from "./pp20.js";

export { isHfe, parseHfe } from "./hfe.js";
export type { HfeHeader, HfeImage } from "./hfe.js";
export { ADF_SIZE, decodeAmigaDisk, decodeAmigaTrack } from "./mfm.js";
export type { DiskDecode, TrackDecode } from "./mfm.js";
export { adfDosType, readOfs } from "./ofs.js";
export type { OfsVolume } from "./ofs.js";
export { decrunchPP20, isPP20 } from "./pp20.js";

export interface IntakeFile {
  name: string;
  data: Uint8Array;
}

export interface IntakeDisk {
  /** Source name: the dropped file or the zip entry, e.g. "Ultima VI Disk 1.hfe". */
  label: string;
  adf: Uint8Array;
  /** AmigaDOS volume name read from the root block, or null if unreadable. */
  volume: string | null;
}

export interface IntakeResult {
  disks: IntakeDisk[];
  /** "VOLUME/dir/file" (or the loose file's own path) → bytes, PP20 already decrunched. */
  files: Map<string, Uint8Array>;
  report: string[];
}

const DISK_EXT = /\.(hfe|adf)$/i;

function isZip(data: Uint8Array): boolean {
  return data.length >= 4 && data[0] === 0x50 && data[1] === 0x4b && (data[2] === 3 || data[2] === 5);
}

function isAdf(name: string, data: Uint8Array): boolean {
  if (adfDosType(data) === null) return false;
  return data.length === ADF_SIZE || data.length === ADF_SIZE * 2 || /\.adf$/i.test(name);
}

function baseName(path: string): string {
  return path.slice(path.lastIndexOf("/") + 1);
}

function normalizePath(name: string): string {
  return name.replace(/\\/g, "/").replace(/^(\.\/|\/)+/, "");
}

function errText(err: unknown): string {
  return err instanceof Error ? err.message : String(err);
}

/** Pull disk images out of a zip; everything else in it (scans, flux streams) is ignored. */
function unzipDisks(name: string, data: Uint8Array, report: string[]): IntakeFile[] {
  const entries = unzipSync(data, {
    filter: (f) => DISK_EXT.test(f.name) && !f.name.startsWith("__MACOSX/") && !baseName(f.name).startsWith("._"),
  });
  const out = Object.entries(entries).map(([entry, bytes]) => ({ name: baseName(entry), data: bytes }));
  report.push(`${name}: zip, ${out.length} disk image(s): ${out.map((f) => f.name).join(", ") || "none"}`);
  return out;
}

function byLabel(a: { name: string }, b: { name: string }): number {
  return a.name.localeCompare(b.name, "en", { numeric: true });
}

export async function intake(files: IntakeFile[]): Promise<IntakeResult> {
  const report: string[] = [];
  const diskInputs: IntakeFile[] = [];
  const loose: IntakeFile[] = [];

  for (const f of files) {
    if (isZip(f.data)) {
      try {
        diskInputs.push(...unzipDisks(f.name, f.data, report));
      } catch (err) {
        report.push(`${f.name}: zip unreadable: ${errText(err)}`);
      }
    } else if (isHfe(f.data) || isAdf(f.name, f.data)) {
      diskInputs.push(f);
    } else {
      loose.push(f);
    }
  }
  diskInputs.sort(byLabel);

  const disks: IntakeDisk[] = [];
  const raw = new Map<string, Uint8Array>();
  const put = (path: string, bytes: Uint8Array): void => {
    if (raw.has(path)) report.push(`${path}: duplicate path, later copy kept`);
    raw.set(path, bytes);
  };

  for (const f of diskInputs) {
    await Promise.resolve(); // let the caller's UI breathe between disks
    let adf: Uint8Array;
    if (isHfe(f.data)) {
      try {
        const hfe = parseHfe(f.data);
        const h = hfe.header;
        const dec = decodeAmigaDisk(hfe);
        adf = dec.adf;
        report.push(
          `${f.name}: HFE ${h.cylinders} cyl x ${h.sides} sides, ${h.bitRateKbps} kbps, encoding ${h.trackEncoding}; ` +
            `sectors ${dec.found}/${dec.total} good, bad header checksums ${dec.badHeader}, ` +
            `bad data checksums ${dec.badData}, wrong-track headers ${dec.wrongTrack}`,
        );
        if (dec.missing.length > 0) {
          const shown = dec.missing.slice(0, 32).join(",");
          report.push(`${f.name}: missing blocks ${shown}${dec.missing.length > 32 ? ",..." : ""} (zero-filled)`);
        }
      } catch (err) {
        report.push(`${f.name}: HFE unreadable: ${errText(err)}`);
        continue;
      }
    } else {
      adf = f.data;
      report.push(`${f.name}: ADF ${f.data.length} bytes`);
    }

    let volume: string | null = null;
    try {
      const vol = readOfs(adf);
      volume = vol.name;
      for (const [path, bytes] of vol.files) put(`${vol.name}/${path}`, bytes);
      report.push(`${f.name}: volume ${vol.name} (${vol.dosType}), ${vol.files.size} files`);
      for (const w of vol.warnings) report.push(`${f.name}: ${w}`);
    } catch (err) {
      report.push(`${f.name}: filesystem unreadable: ${errText(err)}`);
    }
    disks.push({ label: f.name, adf, volume });
  }

  for (const f of loose) put(normalizePath(f.name), f.data);

  const out = new Map<string, Uint8Array>();
  let packed = 0;
  let failed = 0;
  for (const [path, bytes] of raw) {
    if (!isPP20(bytes)) {
      out.set(path, bytes);
      continue;
    }
    try {
      out.set(path, decrunchPP20(bytes));
      packed++;
    } catch (err) {
      out.set(path, bytes);
      failed++;
      report.push(`${path}: PP20 decrunch failed, kept crunched: ${errText(err)}`);
    }
  }
  report.push(`PP20: decrunched ${packed} file(s)${failed ? `, ${failed} failed` : ""}; ${out.size} file(s) total`);

  return { disks, files: out, report };
}
