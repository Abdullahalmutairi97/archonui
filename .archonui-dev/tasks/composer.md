Display :124. You own src/ui/composer/picker.rs and attach.rs; small edits only in src/ui/composer.rs and src/ui/tiles.rs (re-read before editing; don't touch approval code). Gaps from docs/PARITY_AUDIT.md:
1. Model pins: hover pin toggle at each model row's right (muted/acc glyph) using settings.pins / toggle_pin, max 3 per harness; restore Ctrl+P only if it doesn't conflict with the keymap.
2. Model row sub-line: match chat.md's model popover spec if it shows a second line.
3. Escape stops a running reply when nothing is open and focus is in that tile's composer.
4. Attach popover header/summary only if chat.md's prototype has one.
5. Drag-and-drop files anywhere on a tile attaches to its session (one drop handler on the tile body, reuse add_files).
6. Session info popover (tiles.rs): add the old Details tab's files list (attachments + files the agent touched) and activity log, read-only, in the popover's row style.
Verify on :124 (one real codex turn for Esc-stop). Screenshots .qa/integration/compgap-*.png. cargo test. Report (under 15 lines).
