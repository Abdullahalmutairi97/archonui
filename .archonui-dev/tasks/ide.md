Display :125. You own src/ui/tabs/ide.rs and src/ui/tabs/files.rs. Built-in editor gaps from docs/PARITY_AUDIT.md vs old src/chat/editor_panel.rs + editor_panel/{tree,changes}.rs (read only):
1. Changed-on-disk while dirty → Reload / Keep mine bar under the IDE tab strip (toolbar style).
2. Closing a dirty tab asks Save / Don't save / Cancel (prototype popover style).
3. "Open file…" at the tab strip end (next to Follow agent) → filter list (TextInput + Files-style rows) over the tool folder (or Ctrl+P if free in the keymap).
4. Collapsible "AGENT CHANGES · N" section at the top of the Files tab (section-label style) with Clear; click opens the file.
5. Clicking the tab's dirty dot saves.
Verify on :125 with a scratch repo; simulate agent edits from the shell. Screenshots .qa/integration/idegap-*.png. cargo test. Report (under 15 lines).
