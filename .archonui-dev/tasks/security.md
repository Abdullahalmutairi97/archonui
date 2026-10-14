READ-ONLY SECURITY REVIEW (no source edits, no GUI). Write docs/SECURITY_REVIEW.md (a previous reviewer may have started it — continue/complete it). Cite file:line.
- src/engine/approve.rs: Unix socket path/permissions (0600 in 0700 dir), stale socket, who can connect, request authentication (can another local user or sandboxed process auto-approve?), size limits, timeouts; the --approval-mcp stdio server.
- src/engine/harness/*.rs: argv/env building — injection via titles, model ids, paths, MCP defs, skill names (any `sh -c`?); temp files for --mcp-config/--settings (location, perms, cleanup, secrets world-readable?); OPENCODE_CONFIG_CONTENT.
- src/engine/mcp.rs check() spawning imported commands (only on explicit user action?); plugins.rs install/remove.
- src/ide.rs code-server bind address (127.0.0.1), password/cookie handling, file perms, --reuse-window.
- src/preview.rs Chrome remote-debugging binding, URLs agents can open (file://?), profile dir.
- src/terminal.rs tmux socket perms; src/ui/share.rs exports (secrets?); settings export (secrets stripped?); src/app/notify.rs arg injection; files tab delete/trash (traversal, symlinks).
Each finding: severity, evidence, concrete fix; plus what you checked and found fine. Report: path + high/medium findings.
