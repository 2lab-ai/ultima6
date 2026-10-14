// Minimal newline-delimited JSON-RPC client for `rea mcp` over stdio.
import { spawn } from "node:child_process";
import { createInterface } from "node:readline";

export function startRea({ env = {} } = {}) {
  const child = spawn("/opt/homebrew/bin/rea", ["mcp"], {
    env: { ...process.env, ...env },
    stdio: ["pipe", "pipe", "inherit"],
  });
  const pending = new Map();
  let nextId = 1;
  createInterface({ input: child.stdout }).on("line", (line) => {
    if (!line.trim()) return;
    let msg;
    try { msg = JSON.parse(line); } catch { return; }
    if (msg.id !== undefined && pending.has(msg.id)) {
      const { resolve, reject } = pending.get(msg.id);
      pending.delete(msg.id);
      if (msg.error) reject(new Error(JSON.stringify(msg.error)));
      else resolve(msg.result);
    }
  });
  const send = (method, params) => new Promise((resolve, reject) => {
    const id = nextId++;
    pending.set(id, { resolve, reject });
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", id, method, params }) + "\n");
  });
  const notify = (method, params) =>
    child.stdin.write(JSON.stringify({ jsonrpc: "2.0", method, params }) + "\n");
  return {
    async init() {
      const r = await send("initialize", {
        protocolVersion: "2025-06-18",
        capabilities: {},
        clientInfo: { name: "ultima6-re", version: "0.1.0" },
      });
      notify("notifications/initialized", {});
      return r;
    },
    listTools: () => send("tools/list", {}),
    call: (name, args) => send("tools/call", { name, arguments: args }),
    close: () => { child.stdin.end(); child.kill("SIGTERM"); },
  };
}
