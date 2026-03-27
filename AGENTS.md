# Core Instructions
- **Rule #1:** Update this document using concise bullet points to maintain a working memory of the project vision and overall direction for new chats.
- **Size Constraint (< 64k tokens):** If the document exceeds 64k tokens, you must compact it by removing less important details.

# Instructions (rest of file)

- Make it with WASM so you can instrument it in the browser.

- make sure you document functionality in the API in docstrings so as to ensure good docs for modders out there, with examples.

- model the engine rust api after the lua api for minetest/luanti but just more powerful and with more features, we will have server side mods primarily but also client side mods, all written in rust, mods are sandboxed so they can't hurt the main system.

- WriterMD editing direction: Markdown should render inline while typing in the WASM frontend, closer to MarkText/Typora than a split raw-text preview workflow, with a comfortable larger reading scale.
- Use a real markdown renderer for the visible editing layer in the WASM frontend rather than hand-rolled markdown rendering logic.
- Current editor approach: keep a line-oriented live markdown editor in the WASM frontend, with rendered lines by default, raw markdown revealed per-line when needed for markdown control syntax, and explicit per-line raw toggling.
- Frontend editor rendering direction: use semantic row metadata and CSS-driven structure for compact, MarkText-like layout instead of injecting full block HTML into each editable line.
- Document editing should be plain markdown-first; Git integration is optional and belongs to directory-level workflows rather than automatic file open/save behavior.
- Editor ergonomics direction: keyboard-first editing should work across the whole document, including save shortcuts and block-to-block navigation in the live markdown editor.
- AI UX direction: use a docked chat sidebar with explicit Chat and Edit modes instead of a hidden drawer-style panel.
