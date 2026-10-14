import { startRea } from "./mcp-client.mjs";
const rea = startRea();
await rea.init();
const { tools } = await rea.listTools();
const want = new Set(process.argv.slice(2));
for (const t of tools) {
  if (want.size && !want.has(t.name)) continue;
  console.log(want.size ? JSON.stringify({ name: t.name, input: t.inputSchema }, null, 1) : t.name);
}
rea.close();
