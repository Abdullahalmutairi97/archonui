const task = "Heartbeat for the ArchonUI supervisor. Do not read or change anything in the repository. Reply with exactly: heartbeat — the ArchonUI UI work: check whether anything is still unfinished and continue it.";
// Children need an agent; the model override wins over the scout's own default.
const models = ["opencode-go/deepseek-v4.1-flash", "deepseek/deepseek-flash", "antigravity/gemini-3.8-flash"];
const failures = [];
for (const [ix, model] of models.entries()) {
  try {
    const run = await runs.run("heartbeat-" + ix, { agent: "scout", model, task });
    return { key: "heartbeat", model, ok: run.ok, output: String(run.output || "").slice(0, 200) };
  } catch (e) {
    failures.push(model + ": " + String(e.message || e).slice(0, 140));
  }
}
return { key: "heartbeat", ok: false, failures };
