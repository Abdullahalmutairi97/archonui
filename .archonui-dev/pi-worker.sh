#!/usr/bin/env bash
# Runs one ArchonUI worker task with DeepSeek Flash through the Pi CLI: pi-worker.sh <task-name> [extra instructions]
set -u
name="$1"; shift
extra="${*:-}"
cd /home/archonminipc/ArchonUI
prompt="You are a worker on the ArchonUI project at /home/archonminipc/ArchonUI (Rust/GPUI). Read /home/archonminipc/ArchonUI/.archonui-dev/phase3-brief.md first and follow it exactly (UI rule: what the HTML prototype shows stays exactly as is). Then do the task in /home/archonminipc/ArchonUI/.archonui-dev/tasks/${name}.md. Previous workers may have started it — inspect the current state first. Be honest: only report what you actually did and observed; never claim a check you didn't run; cite real file paths. NEVER read or write the user's real data in ~/.local/share/archonui or ~/.config/archonui; always run the app with ARCHONUI_DATA_DIR/ARCHONUI_CONFIG_DIR pointing into /tmp/archonui-qa-N. Kill only your own processes (via /proc/<pid>/environ checks, never pkill -f). ${extra} End with the report the task file asks for."
exec pi -p --model opencode-go/deepseek-v4.1-flash "$prompt"
