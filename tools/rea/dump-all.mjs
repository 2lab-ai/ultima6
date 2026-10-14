// Decompile every procedure of a raw m68k image through one REA MCP session.
// Usage: node dump-all.mjs <image.bin> <entry 0xhex> <procedures.json> <out_dir> [batch]
import { mkdirSync, readFileSync, writeFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { startRea } from "./mcp-client.mjs";

const [image, entry, procFile, outDir, batchArg] = process.argv.slice(2);
const batch = Number(batchArg ?? 25);
mkdirSync(outDir, { recursive: true });
const procs = JSON.parse(readFileSync(procFile, "utf8")).normalized_result;
const todo = procs.map((p) => p.address).filter((a) => !existsSync(join(outDir, `${a}.json`)));
console.error(`procedures ${procs.length}, todo ${todo.length}`);

const rea = startRea();
await rea.init();
const opened = await rea.call("open_binary", {
  path: image, format: "raw-binary", architecture: "m68k", entry_point: entry, provider_id: "ghidra",
});
if (opened.isError) throw new Error(JSON.stringify(opened.content));

const text = (r) => (r.content ?? []).map((c) => c.text ?? "").join("");
const started = Date.now();
for (let i = 0; i < todo.length; i += batch) {
  const slice = todo.slice(i, i + batch);
  const r = await rea.call("batch_decompile", { addresses: slice });
  const body = r.structuredContent ?? JSON.parse(text(r) || "null");
  if (i === 0) writeFileSync(join(outDir, "_first-batch-raw.json"), JSON.stringify(r, null, 1));
  const items = findItems(body);
  for (const addr of slice) {
    const item = items.get(addr) ?? items.get(normalize(addr));
    writeFileSync(join(outDir, `${addr}.json`), JSON.stringify(item ?? { address: addr, missing: true }, null, 1));
  }
  console.error(`${Math.min(i + batch, todo.length)}/${todo.length} ${((Date.now() - started) / 1000).toFixed(0)}s`);
}
await rea.call("close_binary", {});
rea.close();

function normalize(a) { return "0x" + BigInt(a).toString(16); }
function findItems(body) {
  const map = new Map();
  const visit = (o) => {
    if (Array.isArray(o)) return o.forEach(visit);
    if (o && typeof o === "object") {
      const addr = o.address ?? o.procedure_address ?? o.entry_address;
      if (typeof addr === "string" && ("pseudo_code" in o || "pseudocode" in o || "code" in o || "error" in o)) {
        map.set(normalize(addr), o);
        map.set(addr, o);
      }
      Object.values(o).forEach(visit);
    }
  };
  visit(body);
  return map;
}
