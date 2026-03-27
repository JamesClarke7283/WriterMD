# Core Instructions
- **Rule #1:** Update this document using concise bullet points to maintain a working memory of the project vision and overall direction for new chats.
- **Size Constraint (< 64k tokens):** If the document exceeds 64k tokens, you must compact it by removing less important details.

# Instructions (rest of file)

- Make it with WASM so you can instrument it in the browser.

- make sure you document functionality in the API in docstrings so as to ensure good docs for modders out there, with examples.

- model the engine rust api after the lua api for minetest/luanti but just more powerful and with more features, we will have server side mods primarily but also client side mods, all written in rust, mods are sandboxed so they can't hurt the main system.
