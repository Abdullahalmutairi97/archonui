# Harness mapping

Each agent CLI is a `Harness` in `src/engine/harness/`. This page records how
each one maps ArchonUI's modes to its own flags, and what was checked against a
real run. The code is the source of truth; this page explains the choices.

## Modes

| Mode | Meaning |
| --- | --- |
| Auto | Edits and commands run without asking. |
| Plan | Investigate and propose a plan; no edits. |
| Ask | Answer questions; no edits. |

Plan and Ask both forbid edits. They differ in what the chat asks for: Plan
asks for a written plan, Ask asks for an answer. A harness that cannot tell
them apart maps both to the same flags.

## Flags per harness

| Harness | Auto | Plan | Ask | Status |
| --- | --- | --- | --- | --- |
| Claude Code 2.1.300 | `--permission-mode auto` (Claude's own safety check decides each action; no prompt tool) | `--permission-mode plan` | `--permission-mode dontAsk --disallowedTools=Edit,Write,NotebookEdit,Bash` | Verified live (haiku) |
| Codex CLI 0.154.0 | no sandbox flag (user's config applies) | `-c sandbox_mode="read-only"` + plan preamble | `-c sandbox_mode="read-only"` | Verified live |
| OpenCode 1.18.34 | default `build` agent | `--agent plan` + plan preamble | `--agent plan` | Verified live |
| Pi 1.2.0 | no tool flags | `--tools read,grep,find,ls --no-mcp --append-system-prompt <preamble>` | `--tools read,grep,find,ls --no-mcp` | Verified live |
| Gemini CLI | `--approval-mode auto_edit` | `--approval-mode plan` + plan preamble | `--approval-mode plan` | Not installed; from docs |

The plan preamble is the sentence "Plan only: investigate and propose a plan;
do not modify files." It is added to the prompt for harnesses with no
read-only mode of their own that can be set per turn.

## Effort levels

Pi's thinking levels are `off`, `minimal`, `low`, `medium`, `high`, `xhigh`,
`max` (`pi --help`, and the `thinkingLevelMap` of each model from its RPC
`get_available_models`). ArchonUI lists a model's real levels in the composer and
passes the chosen one to `--thinking`, lowercased. The Settings → Models
"Default effort" control is labelled in title case (`High`), and pi rejects
`--thinking High` (it warns and keeps its default), so the harness lowercases
the level before it reaches argv.

## Evidence

Live runs used a scratch folder under `.qa/`, a prompt asking for a file
named `made.txt`, and `archonui --ask`.

- **Claude.** On 2.1.289 `--permission-mode auto` stalled in `-p` mode
  ("Permission needed"). On 2.1.300 it finishes: a turn that wrote a file and
  ran `ls -la` completed in about 11 s with no denials, with the prompt in argv
  and as stream-json on stdin, so Auto uses `auto`. Adding
  `--permission-prompt-tool` makes `auto` hand every call to that tool, so it
  is not passed. `dontAsk` with
  the write tools disallowed read files and created none. `plan` produced a plan
  and created none. `--disallowedTools` takes several values, so the `=` form is
  required; without it the prompt is swallowed.
- **Codex.** `-s read-only` overrode `sandbox_mode = "danger-full-access"` in the
  user's config and blocked the write. `-s workspace-write` created the file.
  `exec resume` does not accept `-s`, so the sandbox is set with
  `-c sandbox_mode=...`; this blocked the write on a resumed thread too.
  `codex exec` has no `-a` flag; the user's `approval_policy = "never"` keeps
  exec from prompting.
- **OpenCode.** `--agent plan` refused to create the file. OpenCode works in the
  project it finds unless told otherwise, so `--dir <chat folder>` is passed on
  every turn. A run without it wrote into the repository root.
- **Codex, settings.** With `sandbox` on and `ask_network` on and off, `archonui --ask codex` answered `ok` both times, so the `-c sandbox_mode` and `network_access` overrides were accepted.
- **OpenCode, steps.** `OPENCODE_CONFIG_CONTENT='{"agent":{"plan":{"steps":7}}}' opencode debug config` shows `agent.plan.steps` as 7.
- **Claude, settings.** Not run live: the account hit its session limit. Two checks are pending: the `--ask` answer with `sandbox` on, and whether `allowedDomains: ["*"]` opens the sandbox network.
- **Pi.** `--tools read,grep,find,ls` with the plan preamble produced a plan and
  no file. Pi has no permission system, so narrowing the tools is the only
  enforcement. Effort was checked live: `--thinking High` is rejected with
  `Warning: Invalid thinking level "High"`, while `--thinking high` runs, so
  ArchonUI lowercases the level. `auth_status` reads `~/.pi/agent/auth.json` and
  `~/.pi/agent/models.json` only (read-only): a fresh Pi writes an empty
  `auth.json`, which does not count as signed in, and a provider `apiKey` in
  `models.json` does.

## General settings per harness

App settings become launch flags only where a harness has a matching flag.
Everything else is listed as not mapped, so no setting is silently ignored.
Auto keeps its meaning, so Ask and Allow tool permissions change nothing in
Auto. Only Off turns a tool off.

The sandbox is off by default, so the user's own harness config (such as Codex's
`sandbox_mode = "danger-full-access"`) applies unless the setting is turned on.
Max steps defaults to 40 and is always sent. `ask_network` only matters with the
sandbox on. The step limit is not sent to Codex or Pi; Gemini
read none of these settings.

| Setting | Claude Code | Codex CLI | OpenCode | Pi | Gemini |
| --- | --- | --- | --- | --- | --- |
| MCP servers | `--mcp-config <file>` with the enabled servers. Additive: Claude's own servers still load, so a server imported from there cannot be switched off here. | `-c mcp_servers.<name>.*` for every server, with `enabled`. Not run live yet. | `mcp` in `OPENCODE_CONFIG_CONTENT`, with `enabled`. Not run live yet. | `--mcp-config <file>` (pi-mcp-adapter). Additive. Not run live yet. | Not passed. |
| Skill off | `--disallowedTools=Skill(<name>)`. Claude may address plugin skills as `plugin:skill`; not checked. | Not supported. | Not supported. | `--no-skills` and `--skill <path>` for each enabled skill. | Not supported. |
| Tool off | `--disallowedTools` for Read, Grep and Glob, Edit, Write and NotebookEdit, Bash, WebSearch, WebFetch. Browser has no Claude built-in. | Not supported. | Not supported; OpenCode's `permission` keys are not checked. | `--exclude-tools` for read, grep, find, ls, edit, write, bash. | Not supported. |
| Max steps | `--max-turns <n>` in every mode. Hidden flag; accepted by 2.1.289 (checked before; re-run blocked by a session limit). | `agent.<name>.steps` in `OPENCODE_CONFIG_CONTENT`: `build` in Auto, `plan` otherwise. Overrides the user's own `steps` for that agent. Schema: `steps`, with `maxSteps` deprecated. | Not in `codex exec --help`. | Not in `pi --help`. | Not supported. |
| Sandbox (on) | Auto only: `--settings {"sandbox":{"enabled":true,"autoAllowBashIfSandboxed":true}}`. Plan and Ask get no sandbox settings: auto-allow in Plan is not checked live. | Auto only: `-c sandbox_mode="workspace-write"`, which also holds on `exec resume`. Plan and Ask keep `read-only`. | Not supported. | Not supported. | Not supported. |
| Sandbox (off) | Nothing added; the user's config applies. | Nothing added; the user's `sandbox_mode` applies. | Nothing added. | Nothing added. | Nothing added. |
| Ask network (on) | Nothing added; the user's `sandbox.network` settings apply. | Network stays off: `exec` cannot ask. | Not supported. | Not supported. | Not supported. |
| Ask network (off) | `sandbox.network.allowedDomains: ["*"]`. Not verified: a live run was blocked by a session limit. If `*` does not match, the sandbox stays network-blocked. | Auto only: `-c sandbox_workspace_write.network_access=true`. | Not supported. | Not supported. | Not supported. |

Server files are written with the turn at mode 0600 under `archonui/mcp/` next to
the sessions folder, so tokens stay out of the process list.

## Ask permissions

Each tool category (read, edit, grep, run, browser, web, fetch) can be Allow,
Ask or Off. Ask only does something where the harness can pause for an answer.

**Claude Code, Auto mode.** Since 0.1.9 Auto is Claude's own `auto` mode, as
in an interactive Claude Code session: Allow tools go in `--allowedTools`, Off
tools in `--disallowedTools`, and Ask tools in neither list, so Claude's safety
check approves or blocks them. Nothing stops to ask, and the approval route
below is not used by any harness today. It is kept for a harness that can ask.

Before 0.1.9, when any category was Ask, Claude ran with
`--permission-mode default` and `--permission-prompt-tool mcp__archonui__approve`,
so it asked before each use of an Ask tool. ArchonUI serves
the `approve` tool itself: the turn starts `archonui --approval-mcp <socket>
<chat id>` from the per-chat server file `mcp/claude-<chat id>.json`. Each
request goes over a per-process Unix socket to the app, which shows an approval
card above the composer. The socket lives in `$XDG_RUNTIME_DIR/archonui` (0700,
falling back to `approvals` inside the sessions folder, never `/tmp`), is
created 0600, and every call must carry the run's random token, handed to the
child in the server file's `env` (the file is 0600). A call from another user or
without the token is refused, and "Always allow" is only ever set by the card's
button, never by a socket message.

- Allow (Enter in the composer) runs the tool with the input Claude asked for.
- Deny (Esc, or the Deny button) returns "The user denied this action." and
  Claude reports the denial.
- Always allow <tool> for this session allows that tool for the rest of the
  app run in that chat. It is kept in memory and is not saved to settings.
- Requests queue per chat; the card shows "1 of N".
- A request no one answers within 10 minutes is denied with "Timed out".
- A request still waiting when its chat's turn ends is denied, and the card is
  removed.

Plan and Ask modes keep their flags above, so they never prompt.

**Other harnesses.** Codex, OpenCode, Pi and Gemini have no
per-tool prompt ArchonUI can answer, and their flags are unchanged. Ask behaves
as Allow there. Only Off turns a tool off.

Settings note for the Ask option (`src/ui/settings/tools.rs`, under the
BUILT-IN TOOLS rows): "Ask: Claude Code in Auto mode asks in ArchonUI before each
use. Other harnesses have no per-tool prompt, so they run Ask tools as Allow."

## Extensions

| Harness | List | Install and remove | On and off |
| --- | --- | --- | --- |
| Claude Code | `plugin list --json --available`; plugin descriptions from each installed manifest | `plugin install <id>`, `plugin uninstall <id>` | `plugin enable` and `plugin disable` |
| Pi | `pi list` | `pi install <source>`, `pi remove <source>`; no catalog, so none is offered | No off switch. Turning a package on does nothing; turning it off is refused. |
| OpenCode | `plugin` array of `opencode debug config` | Refused: plugins are added to OpenCode's config, which ArchonUI does not edit | Refused for the same reason |

Ids are `<harness>:<spec>`, split at the first colon. Install runs with a 300 s
limit, listings and changes with 60 s.

## Auth and connection checks

- `auth_status` is offline. It checks for an API key environment variable or a
  credential file under the home directory. On macOS, Claude's login is in the
  keychain, so a missing credential file reports `Unknown`, not `Missing`.
  Pi is the exception: it parses `~/.pi/agent/auth.json` (an empty `{}` is not
  signed in) and `~/.pi/agent/models.json` instead of testing file existence.
- `test_connection` sends "Reply with exactly: ok" with the harness defaults in
  Ask mode and gives up after 60 seconds. Exposed as `archonui --test <agent>`.

## Settings

`Settings.harnesses` maps a harness id to `HarnessConfig`:

- `enabled` (default true): off hides the harness from new chats. Existing chats
  still use it.
- `binary`: run this path instead of searching `PATH`.
- `default_model`: the model new chats start with, when no per-agent default is set.

## Adding a harness

1. Add `src/engine/harness/<id>.rs` with a type that implements `Harness`.
2. Add it to `HARNESSES` in `src/engine/harness/mod.rs`.
3. Record its verified flags in the table above, and say which were not run.
