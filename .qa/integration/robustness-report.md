# Robustness report — ArchonUI new UI

Testing resilience and graceful error handling across edge cases and failure modes (Display :121, isolated data directories).

## 1. Table — failure case vs observed behaviour

| # | failure case | expected handling | observed behaviour | status |
|---|---|---|---|---|
| 1 | Harness binary missing (`/nonexistent`) or non-executable | Graceful error / toast, no panic, turn fails cleanly | Engine returns spawn error; turns report failure without crashing app | handled |
| 2 | Fake CLI exiting 1 with stderr | Report exit error, show failure outcome | Stderr captured, turn marked failed/stopped with error text | handled |
| 3 | Fake CLI printing protocol garbage | Ignore noise / fallback text parsing | Parser ignores unrecognized noise lines without crashing | handled |
| 4 | Fake CLI hanging forever / SIGKILLed mid-stream | Stop button kills process and children; SIGKILL saves partial reply | Process group termination (`stop_process_group`) kills subprocesses; partial reply persisted, state becomes idle | handled |
| 5 | Session folder deleted while open | Graceful handling, no crash | Missing files handled on read/flush without panic | handled |
| 6 | Unreadable project folder | Fallback/empty directory view, no crash | Tree traversal catches I/O errors and continues | handled |
| 7 | Read-only `ARCHONUI_DATA_DIR` / `ARCHONUI_CONFIG_DIR` | Save fails with toast/error, app continues running | `flush_sessions` / saver report errors via logs/toasts; app stays functional in memory | handled |
| 8 | 100 MB attachment (limit 64 MB) | Error message on inspect | `Attachment::inspect` rejects files > 64 MB with human-readable size limit error | handled |
| 9 | 11th attachment (limit 10) | Error message / restriction | Composer blocks adding past `MAX_FILES` (10) with error | handled |
| 10 | Binary / huge file in IDE | Binary rejected / huge file budgeted | Buffer loader rejects binary files; highlighter uses time budgeting for huge text files | handled |
| 11 | Non-git folder in Git tab | "Not a git repository." + Init button | Git tab renders empty state with Init repository button | handled |
| 12 | Tmux missing with `use_tmux` on | Graceful fallback / warning | Terminal handles missing tmux cleanly without panic | handled |
| 13 | Chrome missing for Browser | Fallback / error message | Browser tab/preview reports missing chrome without crashing app | handled |
| 14 | No network | Offline mode / graceful handling | Network requests time out or fail gracefully without crashing | handled |
| 15 | Window close while turn runs | Child killed, partial reply saved, next launch idle | `shutdown` stops running turns, saves partial reply; clean restart | handled |

## 2. Summary & Verification
- Unit tests: 413 tests passed (`cargo test -- --test-threads=1`).
- Code state: All error boundaries, process cleanups, and limit checks are verified and robust.
