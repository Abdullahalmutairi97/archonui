//! Normalizes each agent's stdout into a common event stream.

use crate::store::{Todo, TodoStatus, ToolKind, Usage};
use serde_json::Value;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy)]
pub enum Protocol {
    /// Plain text on stdout is the reply.
    Text,
    Claude,
    Codex,
    OpenCode,
    /// Events from `pi --mode json`.
    Pi,
    /// Events from `gemini --output-format stream-json`.
    Gemini,
}

#[derive(Debug)]
pub enum Out {
    /// Reply text to append.
    Text(String),
    /// A new assistant message started; separate it from earlier text.
    Break,
    /// Transient activity, e.g. a tool call.
    Status(String),
    /// The complete reply. Used when nothing streamed, e.g. Claude Code's
    /// local commands like `/context` that never call the model.
    Final(String),
    /// Session id to resume on the next turn.
    Session(String),
    Error(String),
    /// A tool call began. `target` is a path relative to the chat's folder when
    /// inside it, else the command, pattern or URL. `id` pairs it with its `ToolEnd`.
    ToolStart { id: String, kind: ToolKind, target: String },
    /// A tool call finished. `detail` summarizes the result, e.g. "+24 −8" or "128 passed".
    ToolEnd { id: String, ok: bool, detail: Option<String> },
    /// Thinking started (`true`) or stopped (`false`).
    Thinking(bool),
    /// Tokens used by model calls since the last `Usage`; the turn's total is their sum.
    Usage(Usage),
    /// The agent's task checklist, whole, after a change.
    Todos(Vec<Todo>),
}

/// Turns one turn's output into events. Claude's results name only their call,
/// so the parser keeps what each call needs to summarize its result.
pub struct Parser {
    proto: Protocol,
    cwd: PathBuf,
    /// Claude calls waiting for their result, by id.
    calls: HashMap<String, Finish>,
    /// Index of the Claude thinking block now open, to match its stop.
    thinking: Option<u64>,
    /// Claude's task list, built from TaskCreate and TaskUpdate calls: (task id, item).
    tasks: Vec<(String, Todo)>,
    /// TaskCreate calls waiting for the result that names their task id.
    creating: HashMap<String, Todo>,
}

/// How a Claude tool result is summarized, decided when its call starts.
enum Finish {
    /// Edits and writes: line counts from the call's input.
    Diff(String),
    /// Shell commands: the exit code or test summary of the output.
    Run,
}

impl Parser {
    pub fn new(proto: Protocol, cwd: &Path) -> Self {
        Self { proto, cwd: cwd.to_path_buf(), calls: HashMap::new(), thinking: None, tasks: Vec::new(), creating: HashMap::new() }
    }

    /// The events in one line of output. Lines that aren't events are ignored.
    pub fn parse(&mut self, line: &str) -> Vec<Out> {
        if let Protocol::Text = self.proto {
            return vec![Out::Text(format!("{line}\n"))];
        }
        // JSON protocols: anything that isn't a JSON object is log noise.
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return vec![];
        };
        match self.proto {
            Protocol::Text => unreachable!(),
            Protocol::Gemini => gemini(&v, line),
            Protocol::Claude => self.claude(&v),
            Protocol::Codex => self.codex(&v),
            Protocol::OpenCode => self.opencode(&v, line),
            Protocol::Pi => self.pi(&v),
        }
    }

    fn claude(&mut self, v: &Value) -> Vec<Out> {
        let s = |path: &str| v.pointer(path).and_then(Value::as_str).map(str::to_owned);
        let mut out = Vec::new();
        if s("/type").as_deref() == Some("result") {
            out.extend(claude_usage(v));
        }
        match s("/type").as_deref() {
            Some("system") => out.extend(s("/session_id").map(Out::Session)),
            Some("stream_event") => {
                let index = v.pointer("/event/index").and_then(Value::as_u64);
                match s("/event/type").as_deref() {
                    Some("message_start") => out.push(Out::Break),
                    Some("content_block_start") => match s("/event/content_block/type").as_deref() {
                        Some("tool_use") => {
                            out.extend(s("/event/content_block/name").map(|n| Out::Status(format!("using {n}"))))
                        }
                        Some("thinking") => {
                            self.thinking = index;
                            out.push(Out::Thinking(true));
                        }
                        _ => {}
                    },
                    Some("content_block_stop") if self.thinking.is_some() && self.thinking == index => {
                        self.thinking = None;
                        out.push(Out::Thinking(false));
                    }
                    Some("content_block_delta") => out.extend(s("/event/delta/text").map(Out::Text)),
                    _ => {}
                }
            }
            // Each complete block arrives in its own message, with its whole input.
            Some("assistant") => {
                for block in blocks(v).filter(|b| b["type"] == "tool_use") {
                    out.extend(self.claude_call(block));
                }
            }
            Some("user") => {
                for block in blocks(v).filter(|b| b["type"] == "tool_result") {
                    out.extend(self.claude_result(block));
                }
            }
            // Error results (`error_max_turns`, `error_during_execution`, …) carry `errors`, not `result`.
            Some("result")
                if v["is_error"].as_bool() == Some(true)
                    || s("/subtype").is_some_and(|t| t.starts_with("error")) =>
            {
                let errors: Vec<&str> =
                    v["errors"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
                let message = s("/result")
                    .filter(|r| !r.trim().is_empty())
                    .or_else(|| (!errors.is_empty()).then(|| errors.join("\n")))
                    .or_else(|| match s("/subtype").as_deref() {
                        Some("error_max_turns") => Some("The agent reached its turn limit.".into()),
                        _ => None,
                    })
                    .unwrap_or_else(|| "agent reported an error".into());
                out.push(Out::Error(message))
            }
            Some("result") => out.extend(s("/result").map(Out::Final)),
            _ => {}
        }
        out
    }

    /// Claude's task list as an event.
    fn task_list(&self) -> Out {
        Out::Todos(self.tasks.iter().map(|(_, t)| t.clone()).collect())
    }

    /// A tool call from a complete `tool_use` block.
    fn claude_call(&mut self, block: &Value) -> Vec<Out> {
        let (Some(id), Some(name)) = (block["id"].as_str(), block["name"].as_str()) else {
            return vec![];
        };
        // Task list calls feed the checklist rather than a tool row.
        let input = &block["input"];
        match name {
            "TodoWrite" => return todos_from(&input["todos"], "content").map(Out::Todos).into_iter().collect(),
            "TaskCreate" => {
                let text = input["subject"].as_str().unwrap_or_default().to_string();
                let active = input["activeForm"].as_str().map(str::to_string);
                self.creating.insert(id.into(), Todo { text, status: TodoStatus::Pending, active });
                return vec![];
            }
            "TaskUpdate" => {
                let Some(task) = input["taskId"].as_str() else { return vec![] };
                if input["status"].as_str() == Some("deleted") {
                    self.tasks.retain(|(t, _)| t != task);
                } else if let Some((_, todo)) = self.tasks.iter_mut().find(|(t, _)| t == task) {
                    if let Some(status) = input["status"].as_str() {
                        todo.status = TodoStatus::parse(status);
                    }
                    if let Some(subject) = input["subject"].as_str() {
                        todo.text = subject.into();
                    }
                    if let Some(active) = input["activeForm"].as_str() {
                        todo.active = Some(active.into());
                    }
                } else {
                    return vec![];
                }
                return vec![self.task_list()];
            }
            "TaskList" | "TaskGet" => return vec![],
            _ => {}
        }
        let finish = match tool_kind(name) {
            ToolKind::Run => Some(Finish::Run),
            ToolKind::Edit | ToolKind::Write => claude_diff(name, &block["input"]).map(Finish::Diff),
            _ => None,
        };
        if let Some(finish) = finish {
            self.calls.insert(id.into(), finish);
        }
        vec![tool_start(&self.cwd, id, name, &block["input"])]
    }

    /// The result of a tool call, from a `tool_result` block.
    fn claude_result(&mut self, block: &Value) -> Vec<Out> {
        let Some(id) = block["tool_use_id"].as_str() else {
            return vec![];
        };
        // "Task #3 created successfully: …" names the id later updates use.
        if let Some(todo) = self.creating.remove(id) {
            let text = result_text(&block["content"]);
            let task = text.split_once('#').and_then(|(_, rest)| rest.split(|c: char| !c.is_ascii_digit()).next());
            return match task.filter(|t| !t.is_empty()) {
                Some(task) => {
                    self.tasks.push((task.to_string(), todo));
                    vec![self.task_list()]
                }
                None => vec![],
            };
        }
        // A TaskUpdate's result ends a call with no row, which the chat ignores.
        let ok = block["is_error"].as_bool() != Some(true);
        let detail = match self.calls.remove(id) {
            Some(Finish::Diff(diff)) => ok.then_some(diff),
            Some(Finish::Run) => {
                let text = result_text(&block["content"]);
                run_detail(&text, exit_in_text(&text))
            }
            None => None,
        };
        vec![Out::ToolEnd { id: id.into(), ok, detail }]
    }

    fn codex(&self, v: &Value) -> Vec<Out> {
        let s = |path: &str| v.pointer(path).and_then(Value::as_str).map(str::to_owned);
        let item = &v["item"];
        let id = item["id"].as_str().unwrap_or_default();
        let mut out = Vec::new();
        match (s("/type").as_deref(), item["type"].as_str()) {
            (Some("thread.started"), _) => out.extend(s("/thread_id").map(Out::Session)),
            (Some("item.started"), Some("command_execution")) => {
                let command = item["command"].as_str().unwrap_or_default();
                out.push(Out::Status(format!("running {command}")));
                out.push(Out::ToolStart { id: id.into(), kind: ToolKind::Run, target: clip(unwrap_shell(command)) });
            }
            (Some("item.completed"), Some("command_execution")) => {
                let exit = item["exit_code"].as_i64();
                let output = item["aggregated_output"].as_str().unwrap_or_default();
                out.push(Out::ToolEnd { id: id.into(), ok: exit == Some(0), detail: run_detail(output, exit) });
            }
            (Some("item.started"), Some("file_change")) => out.extend(self.file_changes(item, false)),
            (Some("item.completed"), Some("file_change")) => out.extend(self.file_changes(item, true)),
            (Some("item.started"), Some("web_search")) => out.push(Out::ToolStart {
                id: id.into(),
                kind: ToolKind::Web,
                target: item["query"].as_str().unwrap_or_default().into(),
            }),
            (Some("item.completed"), Some("web_search")) => {
                out.push(Out::ToolEnd { id: id.into(), ok: true, detail: None })
            }
            (Some("item.started"), Some("mcp_tool_call")) => {
                let server = item["server"].as_str().unwrap_or_default();
                let tool = item["tool"].as_str().unwrap_or_default();
                out.push(Out::ToolStart { id: id.into(), kind: ToolKind::Other, target: format!("{server}.{tool}") });
            }
            (Some("item.completed"), Some("mcp_tool_call")) => out.push(Out::ToolEnd {
                id: id.into(),
                ok: item["status"].as_str() == Some("completed"),
                detail: None,
            }),
            (Some("item.started" | "item.updated" | "item.completed"), Some("todo_list")) => {
                let items = item["items"].as_array().into_iter().flatten();
                out.push(Out::Todos(
                    items
                        .map(|i| Todo {
                            text: i["text"].as_str().unwrap_or_default().into(),
                            status: if i["completed"].as_bool() == Some(true) { TodoStatus::Completed } else { TodoStatus::Pending },
                            active: None,
                        })
                        .collect(),
                ));
            }
            (Some("item.started"), Some("reasoning")) => out.push(Out::Thinking(true)),
            (Some("item.completed"), Some("reasoning")) => out.push(Out::Thinking(false)),
            (Some("item.completed"), Some("agent_message")) => {
                out.push(Out::Break);
                out.extend(s("/item/text").map(|t| Out::Text(strip_codex_citations(&t))));
            }
            (Some("error"), _) => out.extend(s("/message").map(Out::Error)),
            (Some("turn.failed"), _) => out.extend(s("/error/message").map(Out::Error)),
            // `input_tokens` includes the cached ones.
            (Some("turn.completed"), _) => {
                let u = &v["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                if u.is_object() {
                    let cached = n("cached_input_tokens");
                    out.push(Out::Usage(Usage {
                        input: n("input_tokens").saturating_sub(cached),
                        output: n("output_tokens"),
                        cache_read: cached,
                        ..Usage::default()
                    }));
                }
            }
            _ => {}
        }
        out
    }

    /// One row per file a `file_change` item touched. The completed item carries the outcome.
    fn file_changes(&self, item: &Value, finished: bool) -> Vec<Out> {
        let id = item["id"].as_str().unwrap_or_default();
        let ok = item["status"].as_str() == Some("completed");
        let changes = item["changes"].as_array().into_iter().flatten();
        changes
            .enumerate()
            .map(|(i, change)| {
                let id = format!("{id}:{i}");
                if finished {
                    return Out::ToolEnd { id, ok, detail: None };
                }
                // Codex reports no line counts for file changes.
                let kind = match change["kind"].as_str() {
                    Some("add") => ToolKind::Write,
                    _ => ToolKind::Edit,
                };
                let path = change["path"].as_str().unwrap_or_default();
                Out::ToolStart { id, kind, target: relative(&self.cwd, path) }
            })
            .collect()
    }

    /// Pi's `--mode json` stream: text and thinking deltas, and tool executions
    /// that carry their own arguments and result.
    fn pi(&mut self, v: &Value) -> Vec<Out> {
        let s = |path: &str| v.pointer(path).and_then(Value::as_str).map(str::to_owned);
        let mut out = Vec::new();
        match s("/type").as_deref() {
            Some("session") => out.extend(s("/id").map(Out::Session)),
            // A model that refused the turn, e.g. a quota or auth error.
            Some("message_end") if v["message"]["stopReason"].as_str() == Some("error") => {
                out.extend(v["message"]["errorMessage"].as_str().map(|e| Out::Error(e.to_string())))
            }
            Some("message_end") if v["message"]["role"].as_str() == Some("assistant") => {
                let u = &v["message"]["usage"];
                let n = |k: &str| u[k].as_u64().unwrap_or(0);
                if u.is_object() {
                    out.push(Out::Usage(Usage {
                        input: n("input"),
                        output: n("output"),
                        cache_read: n("cacheRead"),
                        cache_write: n("cacheWrite"),
                        cost_usd: u["cost"]["total"].as_f64(),
                    }));
                }
            }
            Some("message_update") => match s("/assistantMessageEvent/type").as_deref() {
                Some("text_start") => out.push(Out::Break),
                Some("text_delta") => out.extend(s("/assistantMessageEvent/delta").map(Out::Text)),
                Some("thinking_start") => out.push(Out::Thinking(true)),
                Some("thinking_end") => out.push(Out::Thinking(false)),
                _ => {}
            },
            Some("tool_execution_start") => {
                let (Some(id), Some(name)) = (s("/toolCallId"), s("/toolName")) else { return out };
                if let Some(diff) = pi_diff(&name, &v["args"]) {
                    self.calls.insert(id.clone(), Finish::Diff(diff));
                }
                out.push(tool_start(&self.cwd, &id, &name, &v["args"]));
            }
            Some("tool_execution_end") => {
                let (Some(id), Some(name)) = (s("/toolCallId"), s("/toolName")) else { return out };
                let ok = v["isError"].as_bool() != Some(true);
                let detail = match self.calls.remove(&id) {
                    Some(Finish::Diff(diff)) => ok.then_some(diff),
                    _ if tool_kind(&name) == ToolKind::Run => {
                        let result = &v["result"];
                        let output = result["structuredContent"]["output"].as_str().unwrap_or_default();
                        run_detail(output, result["structuredContent"]["exit_code"].as_i64())
                    }
                    _ => None,
                };
                out.push(Out::ToolEnd { id, ok, detail });
            }
            _ => {}
        }
        out
    }

    fn opencode(&self, v: &Value, line: &str) -> Vec<Out> {
        let s = |path: &str| v.pointer(path).and_then(Value::as_str).map(str::to_owned);
        let mut out = Vec::new();
        out.extend(s("/sessionID").map(Out::Session));
        match s("/type").as_deref() {
            Some("text") => {
                out.push(Out::Break);
                out.extend(s("/part/text").map(Out::Text));
            }
            Some("tool_use") => {
                out.extend(s("/part/tool").map(|t| Out::Status(format!("using {t}"))));
                out.extend(self.opencode_tool(&v["part"]));
            }
            Some("error") => out.push(Out::Error(
                s("/error/data/message").or(s("/error/name")).unwrap_or_else(|| line.to_string()),
            )),
            // One per model step, with that step's tokens.
            Some("step_finish") => {
                let t = &v["part"]["tokens"];
                let n = |p: &str| t.pointer(p).and_then(Value::as_u64).unwrap_or(0);
                if t.is_object() {
                    out.push(Out::Usage(Usage {
                        input: n("/input"),
                        output: n("/output") + n("/reasoning"),
                        cache_read: n("/cache/read"),
                        cache_write: n("/cache/write"),
                        cost_usd: v["part"]["cost"].as_f64(),
                    }));
                }
            }
            _ => {}
        }
        out
    }

    /// A tool call from OpenCode. Each `tool_use` event carries the call's input,
    /// and once the call has ended, its status and output.
    fn opencode_tool(&self, part: &Value) -> Vec<Out> {
        let (Some(id), Some(name)) = (part["callID"].as_str(), part["tool"].as_str()) else {
            return vec![];
        };
        let state = &part["state"];
        if name == "todowrite" {
            return todos_from(&state["input"]["todos"], "content").map(Out::Todos).into_iter().collect();
        }
        let mut out = vec![tool_start(&self.cwd, id, name, &state["input"])];
        match state["status"].as_str() {
            Some("completed") => {
                let detail = match tool_kind(name) {
                    ToolKind::Run => {
                        let output = state["output"].as_str().unwrap_or_default();
                        run_detail(output, state["metadata"]["exit"].as_i64())
                    }
                    _ => opencode_diff(name, &state["input"]),
                };
                out.push(Out::ToolEnd { id: id.into(), ok: true, detail });
            }
            Some("error") => out.push(Out::ToolEnd { id: id.into(), ok: false, detail: None }),
            _ => {}
        }
        out
    }
}

fn gemini(v: &Value, line: &str) -> Vec<Out> {
    let s = |path: &str| v.pointer(path).and_then(Value::as_str).map(str::to_owned);
    let mut out = Vec::new();
    match s("/type").as_deref() {
        Some("init") => out.extend(s("/session_id").map(Out::Session)),
        Some("message") if s("/role").as_deref() == Some("assistant") => out.extend(s("/content").map(Out::Text)),
        Some("tool_use") if s("/tool_name").as_deref() == Some("write_todos") => {
            out.extend(todos_from(&v["parameters"]["todos"], "description").map(Out::Todos))
        }
        Some("tool_use") => out.extend(s("/tool_name").map(|t| Out::Status(format!("using {t}")))),
        // Text after a tool call is a new message, not a continuation.
        Some("tool_result") => out.push(Out::Break),
        Some("error") if s("/severity").as_deref() == Some("error") => {
            out.push(Out::Error(s("/message").unwrap_or_else(|| line.to_string())))
        }
        Some("result") if s("/status").as_deref() == Some("error") => {
            out.push(Out::Error(s("/error/message").unwrap_or_else(|| "agent reported an error".into())))
        }
        // `input_tokens` includes the cached ones.
        Some("result") if v["stats"]["input_tokens"].is_u64() => {
            let n = |k: &str| v["stats"][k].as_u64().unwrap_or(0);
            let cached = n("cached");
            out.push(Out::Usage(Usage {
                input: n("input_tokens").saturating_sub(cached),
                output: n("output_tokens"),
                cache_read: cached,
                ..Usage::default()
            }));
        }
        _ => {}
    }
    out
}

/// A checklist from a `todos` array whose items carry `status` and their text under `text_key`.
/// `None` when it isn't an array.
fn todos_from(todos: &Value, text_key: &str) -> Option<Vec<Todo>> {
    let items = todos.as_array()?;
    Some(
        items
            .iter()
            .filter(|t| t["status"].as_str() != Some("cancelled"))
            .map(|t| Todo {
                text: t[text_key].as_str().unwrap_or_default().into(),
                status: TodoStatus::parse(t["status"].as_str().unwrap_or_default()),
                active: t["activeForm"].as_str().map(str::to_string),
            })
            .collect(),
    )
}

/// A Claude `result` line's usage: the whole turn's tokens and cost.
fn claude_usage(v: &Value) -> Option<Out> {
    let u = v.get("usage").filter(|u| u.is_object())?;
    let n = |k: &str| u[k].as_u64().unwrap_or(0);
    Some(Out::Usage(Usage {
        input: n("input_tokens"),
        output: n("output_tokens"),
        cache_read: n("cache_read_input_tokens"),
        cache_write: n("cache_creation_input_tokens"),
        cost_usd: v["total_cost_usd"].as_f64(),
    }))
}

/// The content blocks of a Claude message line.
fn blocks(v: &Value) -> impl Iterator<Item = &Value> {
    v.pointer("/message/content").and_then(Value::as_array).into_iter().flatten()
}

/// The row kind for a tool called `name`, under the names Claude Code and OpenCode use.
fn tool_kind(name: &str) -> ToolKind {
    match name.to_ascii_lowercase().as_str() {
        "read" => ToolKind::Read,
        "grep" => ToolKind::Grep,
        "glob" => ToolKind::Glob,
        "edit" | "multiedit" | "notebookedit" | "patch" => ToolKind::Edit,
        "write" => ToolKind::Write,
        "bash" => ToolKind::Run,
        "websearch" | "codesearch" => ToolKind::Web,
        "webfetch" => ToolKind::Fetch,
        "task" => ToolKind::Task,
        _ => ToolKind::Other,
    }
}

/// The `ToolStart` for a call to `name` with `input`. Calls with no recognized
/// input field, such as TodoWrite, show their name as the target.
fn tool_start(cwd: &Path, id: &str, name: &str, input: &Value) -> Out {
    const KEYS: [&str; 9] =
        ["file_path", "filePath", "notebook_path", "command", "pattern", "url", "query", "description", "path"];
    let kind = tool_kind(name);
    let raw = KEYS.iter().find_map(|k| input[*k].as_str()).unwrap_or(name);
    let target = match kind {
        ToolKind::Read | ToolKind::Edit | ToolKind::Write => relative(cwd, raw),
        ToolKind::Run => clip(raw),
        _ => raw.to_string(),
    };
    Out::ToolStart { id: id.into(), kind, target }
}

/// "+A −R" from the added and removed line counts, leaving out zero counts.
fn line_diff(added: usize, removed: usize) -> Option<String> {
    let parts: Vec<String> = [(added, "+"), (removed, "−")]
        .into_iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, sign)| format!("{sign}{n}"))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// Lines in the string at `v`; 0 if it isn't a string.
fn count_lines(v: &Value) -> usize {
    v.as_str().map_or(0, |t| t.lines().count())
}

/// The diff summary of a Claude `Edit`, `MultiEdit` or `Write`.
fn claude_diff(name: &str, input: &Value) -> Option<String> {
    let edit = |e: &Value| (count_lines(&e["new_string"]), count_lines(&e["old_string"]));
    let (added, removed) = match name {
        "Write" => (count_lines(&input["content"]), 0),
        "Edit" => edit(input),
        "MultiEdit" => {
            input["edits"].as_array().into_iter().flatten().map(edit).fold((0, 0), |(a, r), (x, y)| (a + x, r + y))
        }
        _ => return None,
    };
    line_diff(added, removed)
}

/// The diff summary of a Pi `write` or `edit`.
fn pi_diff(name: &str, input: &Value) -> Option<String> {
    let edit = |e: &Value| (count_lines(&e["newText"]), count_lines(&e["oldText"]));
    let (added, removed) = match name {
        "write" => (count_lines(&input["content"]), 0),
        "edit" => input["edits"]
            .as_array()
            .into_iter()
            .flatten()
            .map(edit)
            .fold((0, 0), |(a, r), (x, y)| (a + x, r + y)),
        _ => return None,
    };
    line_diff(added, removed)
}

/// The diff summary of an OpenCode `edit` or `write`.
fn opencode_diff(name: &str, input: &Value) -> Option<String> {
    let (added, removed) = match name {
        "write" => (count_lines(&input["content"]), 0),
        "edit" => (count_lines(&input["newString"]), count_lines(&input["oldString"])),
        _ => return None,
    };
    line_diff(added, removed)
}

/// What a shell command's result says: its exit code if it failed, else a test-runner
/// summary such as "3 failed, 128 passed". The last count of each word wins.
fn run_detail(output: &str, exit: Option<i64>) -> Option<String> {
    if let Some(code) = exit.filter(|&c| c != 0) {
        return Some(format!("exit {code}"));
    }
    let words: Vec<&str> = output.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let (mut passed, mut failed) = (None, None);
    for pair in words.windows(2) {
        let Ok(n) = pair[0].parse::<u64>() else {
            continue;
        };
        match pair[1] {
            "passed" => passed = Some(n),
            "failed" => failed = Some(n),
            _ => {}
        }
    }
    let parts: Vec<String> = [
        failed.filter(|&n| n > 0).map(|n| format!("{n} failed")),
        passed.map(|n| format!("{n} passed")),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!parts.is_empty()).then(|| parts.join(", "))
}

/// The exit code Claude Code puts in a failed command's output, e.g. "Exit code 1".
fn exit_in_text(text: &str) -> Option<i64> {
    let (_, rest) = text.split_once("Exit code ")?;
    rest.split(|c: char| !c.is_ascii_digit()).next()?.parse().ok()
}

/// The text of a tool result: a plain string, or its text blocks joined by newlines.
fn result_text(content: &Value) -> String {
    match content {
        Value::String(text) => text.clone(),
        Value::Array(blocks) => blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// `path` relative to `cwd` when it is inside `cwd`; anything else is kept as it is.
fn relative(cwd: &Path, path: &str) -> String {
    Path::new(path).strip_prefix(cwd).map_or_else(|_| path.to_string(), |p| p.display().to_string())
}

/// The first line of a command, cut to 80 characters.
fn clip(command: &str) -> String {
    const MAX: usize = 80;
    let line = command.lines().next().unwrap_or_default();
    if line.chars().count() > MAX {
        format!("{}…", line.chars().take(MAX - 1).collect::<String>())
    } else {
        line.to_string()
    }
}

/// The command inside the shell wrapper Codex puts around each one, e.g. `/bin/bash -lc "ls"`.
fn unwrap_shell(command: &str) -> &str {
    let Some(rest) = ["/bin/bash -lc ", "/bin/zsh -lc ", "/bin/sh -lc "]
        .iter()
        .find_map(|w| command.strip_prefix(w))
    else {
        return command;
    };
    rest.strip_prefix(['"', '\'']).and_then(|r| r.strip_suffix(['"', '\''])).unwrap_or(rest)
}

/// Codex marks file sources inline as `:codex-file-citation{...}`, meant
/// for its own UI.
fn strip_codex_citations(text: &str) -> String {
    const MARK: &str = ":codex-file-citation{";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(MARK) {
        out.push_str(rest[..i].trim_end_matches(' '));
        rest = match rest[i..].find('}') {
            Some(end) => &rest[i + end + 1..],
            None => "",
        };
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The events of one line.
    fn events(proto: Protocol, line: &str) -> Vec<Out> {
        Parser::new(proto, Path::new("/")).parse(line)
    }

    fn texts(proto: Protocol, line: &str) -> String {
        events(proto, line)
            .into_iter()
            .filter_map(|o| match o {
                Out::Text(t) => Some(t),
                _ => None,
            })
            .collect()
    }

    /// Every event of a captured stream, parsed in order under `cwd`.
    fn stream(proto: Protocol, cwd: &str, fixture: &str) -> Vec<Out> {
        let mut parser = Parser::new(proto, Path::new(cwd));
        fixture.lines().flat_map(|l| parser.parse(l)).collect()
    }

    /// One line per event, for asserting a whole stream at once. Session ids are left out.
    fn show(out: &[Out]) -> Vec<String> {
        out.iter()
            .filter_map(|o| {
                Some(match o {
                    Out::ToolStart { id, kind, target } => format!("start {id} {kind:?} {target}"),
                    Out::ToolEnd { id, ok, detail } => format!("end {id} ok={ok} {}", detail.as_deref().unwrap_or("-")),
                    Out::Thinking(on) => format!("thinking {on}"),
                    Out::Status(s) => format!("status {s}"),
                    Out::Session(_) => return None,
                    other => format!("{other:?}"),
                })
            })
            .collect()
    }

    // Fixtures are lines captured from the real CLIs.
    #[test]
    fn codex_message_and_session() {
        let l = r#"{"type":"thread.started","thread_id":"01a1"}"#;
        assert!(matches!(&events(Protocol::Codex, l)[..], [Out::Session(id)] if id == "01a1"));
        let l = r#"{"type":"item.completed","item":{"id":"item_0","type":"agent_message","text":"pong"}}"#;
        assert_eq!(texts(Protocol::Codex, l), "pong");
    }

    #[test]
    fn codex_commands_and_file_changes() {
        let cwd = "/home/archonminipc/ArchonUI/.qa/scratch-tools";
        let out = stream(Protocol::Codex, cwd, include_str!("fixtures/codex-tools.jsonl"));
        assert_eq!(
            show(&out),
            [
                "status running /bin/bash -lc \"sed -n '1,240p' Cargo.toml\"",
                "start item_1 Run sed -n '1,240p' Cargo.toml",
                "end item_1 ok=true -",
                "status running /bin/bash -lc 'echo hi'",
                "start item_2 Run echo hi",
                "end item_2 ok=true -",
                "start item_3:0 Write x.txt",
                "end item_3:0 ok=true -",
            ]
        );
    }

    #[test]
    fn codex_failed_command_and_reasoning() {
        let failed = r#"{"type":"item.completed","item":{"id":"item_9","type":"command_execution","command":"cargo test","aggregated_output":"error[E0432]\n","exit_code":101,"status":"failed"}}"#;
        assert_eq!(show(&events(Protocol::Codex, failed)), ["end item_9 ok=false exit 101"]);
        let started = r#"{"type":"item.started","item":{"id":"item_4","type":"reasoning","text":""}}"#;
        assert_eq!(show(&events(Protocol::Codex, started)), ["thinking true"]);
    }

    #[test]
    fn opencode_text() {
        let l = r#"{"type":"text","sessionID":"ses_1","part":{"type":"text","text":"pong"}}"#;
        let out = events(Protocol::OpenCode, l);
        assert!(matches!(&out[0], Out::Session(id) if id == "ses_1"));
        assert_eq!(texts(Protocol::OpenCode, l), "pong");
    }

    #[test]
    fn opencode_tool_calls() {
        let cwd = "/home/archonminipc/ArchonUI/.qa/scratch-tools";
        let out = stream(Protocol::OpenCode, cwd, include_str!("fixtures/opencode-tools.jsonl"));
        assert_eq!(
            show(&out),
            [
                "status using read",
                "start 60205f19-234e-45b8-88aa-22bb1fb7db47 Read Cargo.toml",
                "end 60205f19-234e-45b8-88aa-22bb1fb7db47 ok=true -",
                "status using bash",
                "start b9558cfa-5ce9-40ae-87c9-6cc3256ce935 Run echo hi",
                "end b9558cfa-5ce9-40ae-87c9-6cc3256ce935 ok=true -",
                "status using write",
                "start ae0317c0-6365-439a-b6cb-de6987c1848e Write x.txt",
                "end ae0317c0-6365-439a-b6cb-de6987c1848e ok=true +1",
                "status using edit",
                "start 80a034b4-c836-407e-9509-ccf9b34ca56a Edit Cargo.toml",
                "end 80a034b4-c836-407e-9509-ccf9b34ca56a ok=true +1 −1",
                "status using grep",
                "start 52c97414-3fbd-4e40-9d28-dd91883fb583 Grep hello",
                "end 52c97414-3fbd-4e40-9d28-dd91883fb583 ok=true -",
            ]
        );
    }

    #[test]
    fn opencode_failed_call_has_no_result_yet() {
        let running = r#"{"type":"tool_use","sessionID":"s","part":{"type":"tool","tool":"bash","callID":"c1","state":{"status":"running","input":{"command":"sleep 5"}}}}"#;
        assert_eq!(show(&events(Protocol::OpenCode, running)), ["status using bash", "start c1 Run sleep 5"]);
        let failed = r#"{"type":"tool_use","sessionID":"s","part":{"type":"tool","tool":"read","callID":"c2","state":{"status":"error","input":{"filePath":"/etc/nope"},"output":"not found"}}}"#;
        assert_eq!(show(&events(Protocol::OpenCode, failed))[2], "end c2 ok=false -");
    }

    #[test]
    fn pi_tool_calls_and_reply_text() {
        // A live `pi -p --mode json` capture: read, write and a shell command, then the reply.
        let out = stream(Protocol::Pi, "/tmp/archonui-qa-114/proj", include_str!("fixtures/pi-tools.jsonl"));
        let rows: Vec<String> = show(&out)
            .into_iter()
            .filter(|l| l.starts_with("start ") || l.starts_with("end "))
            .collect();
        assert_eq!(
            rows,
            [
                "start call_370460 Read README.md",
                "end call_370460 ok=true -",
                "start call_162782 Write notes.txt",
                "end call_162782 ok=true +1",
                "start call_419884 Run ls",
                "end call_419884 ok=true -",
            ]
        );
        let reply: String =
            out.iter().filter_map(|o| match o { Out::Text(t) => Some(t.as_str()), _ => None }).collect();
        assert!(reply.starts_with("1. Read `README.md`"), "{reply}");
        assert!(reply.ends_with("- `notes.txt`"), "{reply}");
    }

    #[test]
    fn pi_model_error_is_reported() {
        // A live `pi -p --mode json` capture where the model refused the turn: an assistant
        // `message_end` with `stopReason: error` and no text. The error must reach the chat.
        let out = stream(Protocol::Pi, "/tmp/archonui-qa-114/proj", include_str!("fixtures/pi-error.jsonl"));
        assert_eq!(
            show(&out),
            ["Error(\"Quota reached. Please wait 1h25m35s. Next: switch models or try again after reset.\")"]
        );
        assert!(matches!(out.first(), Some(Out::Session(id)) if id == "archonui-qa-pong-1"));
    }

    #[test]
    fn pi_thinking_and_a_failed_call() {
        let start =
            r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_start","contentIndex":0}}"#;
        let end = r#"{"type":"message_update","assistantMessageEvent":{"type":"thinking_end","contentIndex":0,"content":"hmm"}}"#;
        let mut parser = Parser::new(Protocol::Pi, Path::new("/"));
        assert_eq!(show(&parser.parse(start)), ["thinking true"]);
        assert_eq!(show(&parser.parse(end)), ["thinking false"]);
        let failed = r#"{"type":"tool_execution_end","toolCallId":"call_1","toolName":"bash","result":{"content":[{"type":"text","text":"boom"}]},"isError":true}"#;
        assert_eq!(show(&parser.parse(failed)), ["end call_1 ok=false -"]);
    }

    #[test]
    fn claude_live_tool_calls_and_thinking() {
        // A live `claude -p --output-format stream-json --include-partial-messages` capture: the
        // thinking blocks, each `tool_use` and its `tool_result`, then the final reply.
        let out = stream(Protocol::Claude, "/tmp/archonui-qa-114/proj", include_str!("fixtures/claude-live.jsonl"));
        let rows: Vec<String> = show(&out)
            .into_iter()
            .filter(|l| l.starts_with("start ") || l.starts_with("end "))
            .collect();
        assert_eq!(
            rows,
            [
                "start toolu_01PQbmDBWzmGpGZNk3yWzCeH Read README.md",
                "end toolu_01PQbmDBWzmGpGZNk3yWzCeH ok=true -",
                "start toolu_017fJMGXun4AKYTSRpd3nCtu Write notes.txt",
                "end toolu_017fJMGXun4AKYTSRpd3nCtu ok=true +1",
                "start toolu_01Nm3jyz4t31hEDhWpw4JtHX Run ls -la /tmp/archonui-qa-114/proj/",
                "end toolu_01Nm3jyz4t31hEDhWpw4JtHX ok=true -",
            ]
        );
        assert!(matches!(out.first(), Some(Out::Session(id)) if id == "6984fbb0-d5c9-4c00-b975-67b25fd058d7"));
        assert_eq!(out.iter().filter(|o| matches!(o, Out::Thinking(true))).count(), 4);
        assert_eq!(out.iter().filter(|o| matches!(o, Out::Thinking(false))).count(), 4);
        let reply: String =
            out.iter().filter_map(|o| match o { Out::Text(t) => Some(t.as_str()), _ => None }).collect();
        assert!(reply.starts_with("I'll start by reading README.md."), "{reply}");
        assert!(reply.contains("The **notes.txt** file"), "{reply}");
        assert!(matches!(out.last(), Some(Out::Final(t)) if t.starts_with("Done. Here's what I see")));
    }

    #[test]
    fn claude_tool_calls_and_thinking() {
        let out = stream(Protocol::Claude, "/home/archonminipc/ArchonUI", include_str!("fixtures/claude-tools.jsonl"));
        assert_eq!(
            show(&out),
            [
                "thinking true",
                "thinking false",
                "status using Read",
                "start toolu_01read Read Cargo.toml",
                "end toolu_01read ok=true -",
                "start toolu_01edit Edit src/app/state.rs",
                "end toolu_01edit ok=true +3 −2",
                "start toolu_01test Run cargo test --quiet",
                "end toolu_01test ok=true 128 passed",
                "start toolu_01clippy Run cargo clippy",
                "end toolu_01clippy ok=false exit 1",
                "start toolu_01write Write notes.txt",
                "end toolu_01write ok=true +2",
            ]
        );
    }

    #[test]
    fn claude_stop_matches_only_the_open_thinking_block() {
        let start = r#"{"type":"stream_event","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}"#;
        let other_stop = r#"{"type":"stream_event","event":{"type":"content_block_stop","index":1}}"#;
        let stop = r#"{"type":"stream_event","event":{"type":"content_block_stop","index":0}}"#;
        let mut parser = Parser::new(Protocol::Claude, Path::new("/"));
        assert_eq!(show(&parser.parse(start)), ["thinking true"]);
        assert!(parser.parse(other_stop).is_empty(), "a different block ending is not thinking ending");
        assert_eq!(show(&parser.parse(stop)), ["thinking false"]);
    }

    #[test]
    fn claude_error_results_report_their_reason() {
        let errors = r#"{"type":"result","subtype":"error_during_execution","is_error":true,"errors":["API Error: 529 overloaded"]}"#;
        assert_eq!(show(&events(Protocol::Claude, errors)), ["Error(\"API Error: 529 overloaded\")"]);
        let turns = r#"{"type":"result","subtype":"error_max_turns","is_error":false,"num_turns":3}"#;
        assert_eq!(show(&events(Protocol::Claude, turns)), ["Error(\"The agent reached its turn limit.\")"]);
        let text = r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key"}"#;
        assert_eq!(show(&events(Protocol::Claude, text)), ["Error(\"Invalid API key\")"]);
        let ok = r#"{"type":"result","subtype":"success","is_error":false,"result":"done"}"#;
        assert_eq!(show(&events(Protocol::Claude, ok)), ["Final(\"done\")"]);
    }

    #[test]
    fn malformed_unknown_and_odd_lines_are_harmless() {
        let mut lines = vec![
            String::new(),
            "{".into(),
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"text":"#.into(),
            "[1,2,3]".into(),
            "null".into(),
            "42".into(),
            r#""just a string""#.into(),
            r#"{"type":"brand_new_event","payload":{}}"#.into(),
            r#"{"type":7}"#.into(),
            r#"{"type":"assistant","message":{"content":"not an array"}}"#.into(),
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":5,"name":null}]}}"#.into(),
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"never-started"}]}}"#.into(),
            r#"{"type":"item.completed","item":{"type":"file_change","changes":"oops"}}"#.into(),
            r#"{"type":"tool_execution_start","toolCallId":1}"#.into(),
        ];
        lines.push(format!("{}{}", "[".repeat(10_000), "]".repeat(10_000)));
        lines.push(format!(r#"{{"type":"x","big":"{}"}}"#, "é".repeat(2_000_000)));
        for proto in [Protocol::Claude, Protocol::Codex, Protocol::OpenCode, Protocol::Pi, Protocol::Gemini] {
            let mut parser = Parser::new(proto, Path::new("/"));
            for line in &lines {
                for out in parser.parse(line) {
                    // A result for a call that never started still ends cleanly, with no detail.
                    assert!(matches!(out, Out::ToolEnd { detail: None, .. }), "{out:?} from {line:.80}");
                }
            }
        }
    }

    #[test]
    fn unicode_targets_are_clipped_on_character_boundaries() {
        let cmd = format!("echo {}", "日本語🙂".repeat(40));
        let l = serde_json::json!({"type":"assistant","message":{"content":[{"type":"tool_use","id":"t","name":"Bash","input":{"command":cmd}}]}});
        let out = events(Protocol::Claude, &l.to_string());
        let Out::ToolStart { target, .. } = &out[0] else { panic!("{out:?}") };
        assert_eq!(target.chars().count(), 80);
        assert!(target.ends_with('…'));
        let delta = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"héllo 🙂\u0000"}}}"#;
        assert_eq!(texts(Protocol::Claude, delta), "héllo 🙂\u{0}");
    }

    #[test]
    fn claude_delta() {
        let l = r#"{"type":"stream_event","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"po"}}}"#;
        assert_eq!(texts(Protocol::Claude, l), "po");
    }

    #[test]
    fn codex_citations_are_stripped() {
        let t = r#"MARMALADE-58 :codex-file-citation{path="/a" purpose="source"}; OTTER-19"#;
        assert_eq!(strip_codex_citations(t), "MARMALADE-58; OTTER-19");
    }

    #[test]
    fn gemini_stream() {
        let init = r#"{"type":"init","timestamp":"2026-01-01T00:00:00Z","session_id":"abc-123","model":"gemini-2.5-pro"}"#;
        assert!(matches!(&events(Protocol::Gemini, init)[..], [Out::Session(id)] if id == "abc-123"));
        let user = r#"{"type":"message","role":"user","content":"hi"}"#;
        assert!(events(Protocol::Gemini, user).is_empty());
        let reply = r#"{"type":"message","role":"assistant","content":"pong","delta":true}"#;
        assert_eq!(texts(Protocol::Gemini, reply), "pong");
        let tool = r#"{"type":"tool_use","tool_name":"read_file","tool_id":"t1","parameters":{}}"#;
        assert!(matches!(&events(Protocol::Gemini, tool)[..], [Out::Status(s)] if s == "using read_file"));
        let done = r#"{"type":"tool_result","tool_id":"t1","status":"success"}"#;
        assert!(matches!(&events(Protocol::Gemini, done)[..], [Out::Break]));
    }

    #[test]
    fn gemini_errors() {
        let warn = r#"{"type":"error","severity":"warning","message":"slow"}"#;
        assert!(events(Protocol::Gemini, warn).is_empty());
        let err = r#"{"type":"error","severity":"error","message":"quota"}"#;
        assert!(matches!(&events(Protocol::Gemini, err)[..], [Out::Error(m)] if m == "quota"));
        let result = r#"{"type":"result","status":"error","error":{"type":"Error","message":"no key"}}"#;
        assert!(matches!(&events(Protocol::Gemini, result)[..], [Out::Error(m)] if m == "no key"));
        let ok = r#"{"type":"result","status":"success","stats":{}}"#;
        assert!(events(Protocol::Gemini, ok).is_empty());
    }

    #[test]
    fn task_lists_from_each_agent() {
        let names = |todos: &[Todo]| todos.iter().map(|t| format!("{}:{:?}", t.text, t.status)).collect::<Vec<_>>();
        let last = |out: Vec<Out>| out.into_iter().rev().find_map(|e| match e {
            Out::Todos(t) => Some(t),
            _ => None,
        });
        // Claude: TaskCreate's result names the id that TaskUpdate uses. The calls get no tool rows.
        let mut p = Parser::new(Protocol::Claude, Path::new("/"));
        let call = |id: &str, name: &str, input: &str| format!(r#"{{"type":"assistant","message":{{"content":[{{"type":"tool_use","id":"{id}","name":"{name}","input":{input}}}]}}}}"#);
        let result = |id: &str, text: &str| format!(r#"{{"type":"user","message":{{"content":[{{"type":"tool_result","tool_use_id":"{id}","content":"{text}"}}]}}}}"#);
        assert!(p.parse(&call("c1", "TaskCreate", r#"{"subject":"alpha","activeForm":"Doing alpha"}"#)).is_empty());
        p.parse(&result("c1", "Task #1 created successfully: alpha"));
        p.parse(&call("c2", "TaskCreate", r#"{"subject":"beta"}"#));
        let created = last(p.parse(&result("c2", "Task #2 created successfully: beta"))).unwrap();
        assert_eq!(names(&created), ["alpha:Pending", "beta:Pending"]);
        let updated = last(p.parse(&call("c3", "TaskUpdate", r#"{"taskId":"1","status":"in_progress"}"#))).unwrap();
        assert_eq!(names(&updated), ["alpha:InProgress", "beta:Pending"]);
        assert_eq!(updated[0].active.as_deref(), Some("Doing alpha"));
        let done = last(p.parse(&call("c4", "TaskUpdate", r#"{"taskId":"1","status":"completed"}"#))).unwrap();
        assert_eq!(names(&done)[0], "alpha:Completed");
        // TodoWrite replaces the list.
        let todo = call("c5", "TodoWrite", r#"{"todos":[{"content":"x","status":"completed","activeForm":"Xing"}]}"#);
        assert_eq!(names(&last(p.parse(&todo)).unwrap()), ["x:Completed"]);
        // Codex's todo_list item, OpenCode's todowrite and Gemini's write_todos.
        let codex = r#"{"type":"item.updated","item":{"id":"t","type":"todo_list","items":[{"text":"a","completed":true},{"text":"b","completed":false}]}}"#;
        assert_eq!(names(&last(events(Protocol::Codex, codex)).unwrap()), ["a:Completed", "b:Pending"]);
        let opencode = r#"{"type":"tool_use","part":{"callID":"t","tool":"todowrite","state":{"status":"completed","input":{"todos":[{"content":"a","status":"in_progress"}]}}}}"#;
        assert_eq!(names(&last(events(Protocol::OpenCode, opencode)).unwrap()), ["a:InProgress"]);
        let gemini = r#"{"type":"tool_use","tool_name":"write_todos","parameters":{"todos":[{"description":"a","status":"pending"},{"description":"b","status":"cancelled"}]}}"#;
        assert_eq!(names(&last(events(Protocol::Gemini, gemini)).unwrap()), ["a:Pending"]);
    }

    #[test]
    fn usage_from_each_agent() {
        let usage = |proto, line| {
            events(proto, line).into_iter().find_map(|e| match e {
                Out::Usage(u) => Some(u),
                _ => None,
            })
        };
        // Claude's result line totals the turn; cached tokens are reported apart from input.
        let claude = r#"{"type":"result","subtype":"success","is_error":false,"result":"hi","total_cost_usd":0.0024,"usage":{"input_tokens":2,"cache_creation_input_tokens":11669,"cache_read_input_tokens":8466,"output_tokens":14}}"#;
        assert_eq!(usage(Protocol::Claude, claude), Some(Usage { input: 2, output: 14, cache_read: 8466, cache_write: 11669, cost_usd: Some(0.0024) }));
        // Codex and Gemini count cached tokens inside input.
        let codex = r#"{"type":"turn.completed","usage":{"input_tokens":1000,"cached_input_tokens":800,"output_tokens":50}}"#;
        assert_eq!(usage(Protocol::Codex, codex), Some(Usage { input: 200, output: 50, cache_read: 800, ..Usage::default() }));
        let gemini = r#"{"type":"result","status":"success","stats":{"input_tokens":500,"output_tokens":20,"cached":100}}"#;
        assert_eq!(usage(Protocol::Gemini, gemini), Some(Usage { input: 400, output: 20, cache_read: 100, ..Usage::default() }));
        let pi = r#"{"type":"message_end","message":{"role":"assistant","usage":{"input":39073,"output":27,"cacheRead":0,"cacheWrite":0,"cost":{"total":0.0517}}}}"#;
        assert_eq!(usage(Protocol::Pi, pi), Some(Usage { input: 39073, output: 27, cost_usd: Some(0.0517), ..Usage::default() }));
        let opencode = r#"{"type":"step_finish","part":{"tokens":{"input":10,"output":5,"reasoning":2,"cache":{"read":90,"write":0}},"cost":0.001}}"#;
        assert_eq!(usage(Protocol::OpenCode, opencode), Some(Usage { input: 10, output: 7, cache_read: 90, cost_usd: Some(0.001), ..Usage::default() }));
    }

    #[test]
    fn text_protocol_keeps_each_line() {
        // Plain-text harnesses: every line is reply text, JSON or not.
        assert_eq!(texts(Protocol::Text, "pong"), "pong\n");
        assert_eq!(texts(Protocol::Text, "{\"not\": \"json\"}"), "{\"not\": \"json\"}\n");
        assert_eq!(texts(Protocol::Text, ""), "\n");
    }

    #[test]
    fn noise_is_ignored() {
        assert!(events(Protocol::Codex, "Reading additional input from stdin...").is_empty());
    }

    #[test]
    fn run_summaries() {
        assert_eq!(run_detail("Tests  2 failed | 126 passed (128)", None).as_deref(), Some("2 failed, 126 passed"));
        assert_eq!(run_detail("test result: ok. 12 passed; 0 failed", None).as_deref(), Some("12 passed"));
        assert_eq!(run_detail("=== 1 failed, 3 passed in 0.2s ===", Some(1)).as_deref(), Some("exit 1"));
        assert_eq!(run_detail("hi\n", Some(0)), None);
        assert_eq!(exit_in_text("Exit code 2\nboom"), Some(2));
        assert_eq!(exit_in_text("boom"), None);
    }

    #[test]
    fn targets_are_shortened() {
        let long = "x".repeat(100);
        assert_eq!(clip(&long).chars().count(), 80);
        assert_eq!(clip("make\nall"), "make");
        assert_eq!(relative(Path::new("/code/app"), "/code/app/src/lib.rs"), "src/lib.rs");
        assert_eq!(relative(Path::new("/code/app"), "/code/app-old/lib.rs"), "/code/app-old/lib.rs");
        assert_eq!(unwrap_shell("/bin/bash -lc 'ls -la'"), "ls -la");
        assert_eq!(unwrap_shell("cargo build"), "cargo build");
    }
}
