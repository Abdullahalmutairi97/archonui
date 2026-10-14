# E2E report — live multi-harness runs, new UI (display :114, /tmp/archonui-qa-114)

Ran real turns through `target/debug/archonui --new-ui` (isolated `ARCHONUI_DATA_DIR`/`ARCHONUI_CONFIG_DIR`, scratch git proj). Evidence: `.qa/integration/evidence/{codex,opencode,pi,claude}-session.json`, `evidence/argv.txt` (from `/proc/<child>/cmdline`), screenshots `.qa/integration/e2e-*.png`.

| check | codex | opencode | pi |
|---|---|---|---|
| tool rows live | ✓ run/write/run, "Working · 3 tool calls" (e2e-03-run-*) | ✓ read/write +1/run (e2e-22b-run-*) | ✓ read/write +1/run (e2e-45b-run-*); its `ls` row shows kind "other" |
| header thought/worked for Ns | ✓ "worked for 21s/47s/31s" | ✓ "worked for 16s/10s" | ✓ "thought for 1s" (main), "worked for 18s" (ask) |
| reply streams | ✓ | ✓ | ✓ |
| session JSON message/tools/took/thought | ✓ took=21 thought=– | ✓ took=16 thought=– | ✓ took=23 thought=1 |
| Plan mode: no files changed | ✓ read-only sandbox, plan-only.txt absent | ✓ `--agent plan`, big-pickle.txt absent | ✓ write refused, pi-plan.txt absent |
| Ask mode: answers only | ✓ ls only | ✓ answers, no change | ✓ answers, no change |
| Stop mid-turn (idle, partial kept) | ✓ child killed, partial reply + "Stopped." kept (e2e-72) | ✓ killed, "Stopped." (no text yet) | ✓ killed, "Stopped." (no text yet) |
| follow-up reuses CLI session id | ✓ `codex exec resume 01a1346c…` | ✓ `--session ses_ecb8c75c…` | ✓ `--session-id archonui-18de0b34…` |
| picked model/effort → argv | ✓ `--model gpt-5.6-terra -c model_reasoning_effort="max"` | ✓ `--model opencode/big-pickle` | ✓ `--model deepseek/deepseek-flash --thinking high` |
| claude | **not verified live**: account session limit ("resets 1:20pm") hit on Opus and haiku; the app did surface the two error rows correctly (e2e-63, claude-session.json). Tool_use/thinking mapping remains unverified live here; retry scheduled after reset (bg_1812f_mv6i7hss_1). |

**Parser fix.** `src/engine/parse.rs`: pi's failed-model turn (`message_end` with `stopReason:"error"`) now emits `Out::Error(errorMessage)`, so a quota failure shows the real message instead of "The agent finished without replying." Fixture `src/engine/fixtures/pi-error.jsonl` (real capture, `/tmp` paths only) + test `pi_model_error_is_reported`. `engine::parse` 20/20 tests pass. Full suite: 410 passed, 4 failed — all 4 are environmental inotify ENOSPC ("OS file watch limit reached", other workers' apps), reproducible in isolation and unrelated to this change.

**Open issues.** (1) Sidebar "New session" (`src/ui/sidebar/sessions.rs:280`) ignores `harness_defaults`/`default_mode`, so a new claude session starts on Opus, not the configured haiku — outside this task's edit scope. (2) Claude's model picker has no "haiku" entry, so haiku is only reachable via first-session defaults. (3) pi/claude `ls`/`LS` rows map to kind Other ("other ."), unlike the prototype's named kinds. (4) One opencode Ask run failed environmentally (Bad Request + inotify ENOSPC); the retry succeeded.
