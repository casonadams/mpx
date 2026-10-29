import { spawn } from "node:child_process";
import { resolve } from "node:path";

const MPX = __MPX_EXE__;

// Runs `mpx hook omp <event>` with `data` on stdin; resolves to what it prints (context about
// other agents for the model), or "". Outside mpx it does nothing.
function hook(event, data) {
  if (!process.env.MPX_PANE || !process.env.MPX_ENDPOINT) return Promise.resolve("");
  return new Promise((done) => {
    try {
      const child = spawn(MPX, ["hook", "omp", event], { stdio: ["pipe", "pipe", "ignore"] });
      let out = "";
      child.stdout.on("data", (chunk) => { out += chunk; });
      child.on("error", () => done(""));
      child.on("close", () => done(out.trim()));
      child.stdin.on("error", () => {});
      child.stdin.end(JSON.stringify(data));
    } catch { done(""); }
  });
}

function about(ctx, extra) {
  return { session_id: ctx?.sessionManager?.getSessionId?.() ?? null, cwd: ctx?.cwd ?? process.cwd(), ...extra };
}

// The file a tool worked on, absolute: write and edit report it in their details, read in its
// source. None for omp's virtual paths (`xd://...`, through which it runs MCP tools).
function toolPath(event, cwd) {
  const d = event?.details ?? {};
  const path = d.resolvedPath ?? d.path ?? d.meta?.source?.value ?? event?.input?.path;
  if (typeof path !== "string" || /^[a-z][a-z0-9+.-]*:\/\//i.test(path)) return null;
  return resolve(cwd, path);
}

function lastReply(messages) {
  for (const m of [...(messages ?? [])].reverse()) {
    if (m?.role !== "assistant" || !Array.isArray(m.content)) continue;
    const text = m.content.filter((c) => c?.type === "text").map((c) => c.text).join("\n").trim();
    if (text) return text;
  }
  return null;
}

export default function mpxAgentHooks(pi) {
  pi.on("session_start", (_event, ctx) => { hook("session_start", about(ctx)); });
  pi.on("before_agent_start", async (event, ctx) => {
    const context = await hook("before_agent_start", about(ctx, { prompt: event?.prompt ?? null }));
    return context ? { message: { customType: "mpx", content: context, display: false } } : undefined;
  });
  pi.on("agent_start", (_event, ctx) => { hook("agent_start", about(ctx)); });
  pi.on("turn_start", (_event, ctx) => { hook("turn_start", about(ctx)); });
  pi.on("tool_result", async (event, ctx) => {
    const path = toolPath(event, ctx?.cwd ?? process.cwd());
    if (!path) return undefined;
    const context = await hook("tool_result", about(ctx, { tool: event.toolName, path }));
    return context ? { additionalContext: context } : undefined;
  });
  pi.on("tool_approval_requested", (event, ctx) =>
    { hook("tool_approval_requested", about(ctx, { detail: event?.toolName ?? null })); });
  pi.on("tool_approval_resolved", (_event, ctx) => { hook("tool_approval_resolved", about(ctx)); });
  pi.on("agent_end", (event, ctx) => { hook("agent_end", about(ctx, { reply: lastReply(event?.messages) })); });
  pi.on("session_shutdown", (_event, ctx) => { hook("session_shutdown", about(ctx)); });
}
