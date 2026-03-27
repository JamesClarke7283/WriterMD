use leptos::prelude::*;
use leptos::ev;
use serde::{Deserialize, Serialize};
use tauri_wasm::api::core::invoke;
use wasm_bindgen::JsCast;

// ── Tauri command argument/response types ──

#[derive(Serialize)]
struct EmptyArgs {}

#[derive(Serialize)]
struct SaveFileArgs {
    path: String,
    content: String,
}

#[derive(Serialize)]
struct SaveFileAsArgs {
    content: String,
}

#[derive(Debug, Clone, Deserialize)]
struct OpenFileResponse {
    path: String,
    content: String,
}

// ── Helper: extract filename from path ──

fn filename_from_path(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

// ── Markdown insert items ──

/// A markdown syntax item for the @ insert menu.
#[derive(Clone, PartialEq)]
struct InsertItem {
    /// Icon/emoji displayed in the menu
    icon: &'static str,
    /// Label shown to the user
    label: &'static str,
    /// Description/hint text
    description: &'static str,
    /// The markdown text to insert (replaces `@query`)
    insert_text: &'static str,
    /// Where to place cursor relative to start of inserted text. None = end.
    cursor_offset: Option<usize>,
}

/// All available markdown syntax items for the @ menu.
fn get_insert_items() -> Vec<InsertItem> {
    vec![
        // ── Headings ──
        InsertItem {
            icon: "H1",
            label: "Heading 1",
            description: "Top-level heading",
            insert_text: "# ",
            cursor_offset: None,
        },
        InsertItem {
            icon: "H2",
            label: "Heading 2",
            description: "Section heading",
            insert_text: "## ",
            cursor_offset: None,
        },
        InsertItem {
            icon: "H3",
            label: "Heading 3",
            description: "Sub-section heading",
            insert_text: "### ",
            cursor_offset: None,
        },
        InsertItem {
            icon: "H4",
            label: "Heading 4",
            description: "Sub-sub-section heading",
            insert_text: "#### ",
            cursor_offset: None,
        },
        InsertItem {
            icon: "H5",
            label: "Heading 5",
            description: "Minor heading",
            insert_text: "##### ",
            cursor_offset: None,
        },
        InsertItem {
            icon: "H6",
            label: "Heading 6",
            description: "Smallest heading",
            insert_text: "###### ",
            cursor_offset: None,
        },
        // ── Lists ──
        InsertItem {
            icon: "•",
            label: "Bullet List",
            description: "Unordered list item",
            insert_text: "- ",
            cursor_offset: None,
        },
        InsertItem {
            icon: "1.",
            label: "Numbered List",
            description: "Ordered list item",
            insert_text: "1. ",
            cursor_offset: None,
        },
        InsertItem {
            icon: "☐",
            label: "Task List",
            description: "Checkbox item",
            insert_text: "- [ ] ",
            cursor_offset: None,
        },
        // ── Block elements ──
        InsertItem {
            icon: "❝",
            label: "Blockquote",
            description: "Quoted text block",
            insert_text: "> ",
            cursor_offset: None,
        },
        InsertItem {
            icon: "⟨⟩",
            label: "Code Block",
            description: "Fenced code block",
            insert_text: "```\n\n```",
            cursor_offset: Some(4), // after first ``` and newline
        },
        InsertItem {
            icon: "──",
            label: "Horizontal Rule",
            description: "Divider line",
            insert_text: "---\n",
            cursor_offset: None,
        },
        // ── Inline formatting ──
        InsertItem {
            icon: "B",
            label: "Bold",
            description: "Strong emphasis",
            insert_text: "**text**",
            cursor_offset: Some(2), // select "text" between **
        },
        InsertItem {
            icon: "I",
            label: "Italic",
            description: "Emphasis",
            insert_text: "*text*",
            cursor_offset: Some(1),
        },
        InsertItem {
            icon: "S",
            label: "Strikethrough",
            description: "Crossed out text",
            insert_text: "~~text~~",
            cursor_offset: Some(2),
        },
        InsertItem {
            icon: "`",
            label: "Inline Code",
            description: "Code span",
            insert_text: "`code`",
            cursor_offset: Some(1),
        },
        // ── Links & media ──
        InsertItem {
            icon: "🔗",
            label: "Link",
            description: "Hyperlink",
            insert_text: "[text](url)",
            cursor_offset: Some(1), // inside []
        },
        InsertItem {
            icon: "🖼",
            label: "Image",
            description: "Image embed",
            insert_text: "![alt](url)",
            cursor_offset: Some(2), // inside []
        },
        // ── Table ──
        InsertItem {
            icon: "⊞",
            label: "Table",
            description: "Markdown table",
            insert_text: "| Column 1 | Column 2 | Column 3 |\n| -------- | -------- | -------- |\n| Cell     | Cell     | Cell     |\n",
            cursor_offset: None,
        },
    ]
}

// ── App (root component) ──

/// Root component for WriterMD. Manages theme, file state, and layout.
#[component]
pub fn App() -> impl IntoView {
    // Theme: true = dark, false = light
    let (is_dark, set_is_dark) = signal(true);
    // Editor content
    let (content, set_content) = signal(String::new());
    // Current file path (None = untitled)
    let (file_path, set_file_path) = signal::<Option<String>>(None);
    // Dirty flag (unsaved changes)
    let (is_dirty, set_is_dirty) = signal(false);
    // Menu open state
    let (menu_open, set_menu_open) = signal(false);

    // Character count derived from content
    let char_count = Memo::new(move |_| content.get().len());

    // Display name for title bar
    let display_name = Memo::new(move |_| {
        let name = match file_path.get() {
            Some(p) => filename_from_path(&p),
            None => "Untitled-1".to_string(),
        };
        if is_dirty.get() {
            format!("● {}", name)
        } else {
            name
        }
    });

    // ── File operations ──

    let do_open = move || {
        leptos::task::spawn_local(async move {
            let result: Option<OpenFileResponse> =
                invoke("open_file", &EmptyArgs {}).await.unwrap();
            if let Some(resp) = result {
                set_content.set(resp.content);
                set_file_path.set(Some(resp.path));
                set_is_dirty.set(false);
            }
        });
    };

    let do_save_as = move || {
        let text = content.get();
        leptos::task::spawn_local(async move {
            let result: Option<String> =
                invoke("save_file_as", &SaveFileAsArgs { content: text })
                    .await
                    .unwrap();
            if let Some(new_path) = result {
                set_file_path.set(Some(new_path));
                set_is_dirty.set(false);
            }
        });
    };

    let do_save = move || {
        let path = file_path.get();
        let text = content.get();
        match path {
            Some(p) => {
                leptos::task::spawn_local(async move {
                    let _: () = invoke(
                        "save_file",
                        &SaveFileArgs {
                            path: p,
                            content: text,
                        },
                    )
                    .await
                    .unwrap();
                    set_is_dirty.set(false);
                });
            }
            None => {
                do_save_as();
            }
        }
    };

    // ── Window controls (via Tauri commands for v2 compatibility) ──

    let do_minimize = move || {
        leptos::task::spawn_local(async move {
            let _: Result<(), _> = invoke("window_minimize", &EmptyArgs {}).await;
        });
    };

    let do_maximize = move || {
        leptos::task::spawn_local(async move {
            let _: Result<(), _> = invoke("window_toggle_maximize", &EmptyArgs {}).await;
        });
    };

    let do_close = move || {
        leptos::task::spawn_local(async move {
            let _: Result<(), _> = invoke("window_close", &EmptyArgs {}).await;
        });
    };

    // Theme class on body
    Effect::new(move |_| {
        let doc = web_sys::window().unwrap().document().unwrap();
        let body = doc.body().unwrap();
        if is_dark.get() {
            body.set_attribute("data-theme", "dark").unwrap();
        } else {
            body.set_attribute("data-theme", "light").unwrap();
        }
    });

    view! {
        <div class="app-container">
            <TitleBar
                display_name=display_name
                char_count=char_count
                menu_open=menu_open
                set_menu_open=set_menu_open
                on_minimize=do_minimize
                on_maximize=do_maximize
                on_close=do_close
            />
            <MenuOverlay
                is_open=menu_open
                set_is_open=set_menu_open
                is_dark=is_dark
                set_is_dark=set_is_dark
                on_open=do_open
                on_save=do_save
                on_save_as=do_save_as
            />
            <Editor
                content=content
                set_content=set_content
                set_is_dirty=set_is_dirty
            />
        </div>
    }
}

// ── TitleBar ──

/// Custom title bar matching the screenshot design.
#[component]
fn TitleBar(
    display_name: Memo<String>,
    char_count: Memo<usize>,
    menu_open: ReadSignal<bool>,
    set_menu_open: WriteSignal<bool>,
    on_minimize: impl Fn() + 'static + Copy + Send + Sync,
    on_maximize: impl Fn() + 'static + Copy + Send + Sync,
    on_close: impl Fn() + 'static + Copy + Send + Sync,
) -> impl IntoView {
    view! {
        <div class="title-bar" data-tauri-drag-region="true">
            <div class="title-bar-left">
                <button
                    class="title-bar-btn hamburger-btn"
                    on:click=move |_| set_menu_open.set(!menu_open.get())
                    title="Menu"
                >
                    "≡"
                </button>
                <span class="char-count">
                    "C " {move || char_count.get().to_string()}
                </span>
            </div>
            <div class="title-bar-center" data-tauri-drag-region="true">
                <span class="file-name">{move || display_name.get()}</span>
            </div>
            <div class="title-bar-right">
                <button class="title-bar-btn window-btn" on:click=move |_| on_minimize() title="Minimize">
                    "−"
                </button>
                <button class="title-bar-btn window-btn" on:click=move |_| on_maximize() title="Maximize">
                    "□"
                </button>
                <button class="title-bar-btn window-btn close-btn" on:click=move |_| on_close() title="Close">
                    "×"
                </button>
            </div>
        </div>
    }
}

// ── MenuOverlay ──

/// Slide-out menu overlay with file operations and theme toggle.
#[component]
fn MenuOverlay(
    is_open: ReadSignal<bool>,
    set_is_open: WriteSignal<bool>,
    is_dark: ReadSignal<bool>,
    set_is_dark: WriteSignal<bool>,
    on_open: impl Fn() + 'static + Copy + Send + Sync,
    on_save: impl Fn() + 'static + Copy + Send + Sync,
    on_save_as: impl Fn() + 'static + Copy + Send + Sync,
) -> impl IntoView {
    let close_menu = move || set_is_open.set(false);

    view! {
        <Show when=move || is_open.get()>
            <div class="menu-backdrop" on:click=move |_| close_menu()></div>
            <div class="menu-panel">
                <button class="menu-item" on:click=move |_| {
                    close_menu();
                    on_open();
                }>
                    <span class="menu-icon">"📂"</span>
                    <span>"Open"</span>
                </button>
                <button class="menu-item" on:click=move |_| {
                    close_menu();
                    on_save();
                }>
                    <span class="menu-icon">"💾"</span>
                    <span>"Save"</span>
                </button>
                <button class="menu-item" on:click=move |_| {
                    close_menu();
                    on_save_as();
                }>
                    <span class="menu-icon">"📄"</span>
                    <span>"Save As"</span>
                </button>
                <div class="menu-separator"></div>
                <button class="menu-item" on:click=move |_| {
                    set_is_dark.set(!is_dark.get());
                }>
                    <span class="menu-icon">
                        {move || if is_dark.get() { "☀️" } else { "🌙" }}
                    </span>
                    <span>
                        {move || if is_dark.get() { "Light Theme" } else { "Dark Theme" }}
                    </span>
                </button>
            </div>
        </Show>
    }
}

// ── Editor with @ Insert Menu ──

/// Main editing area with @ trigger insert menu for markdown syntax.
#[component]
fn Editor(
    content: ReadSignal<String>,
    set_content: WriteSignal<String>,
    set_is_dirty: WriteSignal<bool>,
) -> impl IntoView {
    // @ menu state
    let (insert_menu_open, set_insert_menu_open) = signal(false);
    let (insert_filter, set_insert_filter) = signal(String::new());
    let (at_trigger_pos, set_at_trigger_pos) = signal::<Option<usize>>(None);
    let (selected_index, set_selected_index) = signal(0usize);
    // Textarea node ref
    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();

    // Filtered insert items
    let filtered_items = Memo::new(move |_| {
        let filter = insert_filter.get().to_lowercase();
        let all_items = get_insert_items();
        if filter.is_empty() {
            all_items
        } else {
            all_items
                .into_iter()
                .filter(|item| {
                    item.label.to_lowercase().contains(&filter)
                        || item.description.to_lowercase().contains(&filter)
                })
                .collect()
        }
    });

    // Insert the selected item into the textarea
    let do_insert = move |item: &InsertItem| {
        if let Some(trigger_pos) = at_trigger_pos.get() {
            let current = content.get();
            let filter_text = insert_filter.get();
            // Range to replace: from '@' to '@' + filter length
            let replace_end = trigger_pos + 1 + filter_text.len();
            let replace_end = replace_end.min(current.len());

            let mut new_content = String::with_capacity(current.len() + item.insert_text.len());
            new_content.push_str(&current[..trigger_pos]);
            new_content.push_str(item.insert_text);
            new_content.push_str(&current[replace_end..]);

            let cursor_pos = if let Some(offset) = item.cursor_offset {
                trigger_pos + offset
            } else {
                trigger_pos + item.insert_text.len()
            };

            set_content.set(new_content);
            set_is_dirty.set(true);
            set_insert_menu_open.set(false);
            set_insert_filter.set(String::new());
            set_at_trigger_pos.set(None);

            // Set cursor position after Leptos re-renders
            let cursor = cursor_pos;
            if let Some(el) = textarea_ref.get() {
                let el: web_sys::HtmlTextAreaElement = el.into();
                // Use requestAnimationFrame to set cursor after DOM update
                let closure = wasm_bindgen::closure::Closure::once(move || {
                    let _ = el.set_selection_range(cursor as u32, cursor as u32);
                    let _ = el.focus();
                });
                web_sys::window()
                    .unwrap()
                    .request_animation_frame(closure.as_ref().unchecked_ref())
                    .unwrap();
                closure.forget();
            }
        }
    };

    // Handle keydown for navigation and selection in the @ menu
    let on_keydown = move |ev: ev::KeyboardEvent| {
        if !insert_menu_open.get() {
            return;
        }
        let items = filtered_items.get();
        let key = ev.key();
        match key.as_str() {
            "ArrowDown" => {
                ev.prevent_default();
                let len = items.len();
                if len > 0 {
                    set_selected_index.set((selected_index.get() + 1) % len);
                }
            }
            "ArrowUp" => {
                ev.prevent_default();
                let len = items.len();
                if len > 0 {
                    let idx = selected_index.get();
                    set_selected_index.set(if idx == 0 { len - 1 } else { idx - 1 });
                }
            }
            "Enter" | "Tab" => {
                ev.prevent_default();
                let idx = selected_index.get();
                if idx < items.len() {
                    let item = items[idx].clone();
                    do_insert(&item);
                }
            }
            "Escape" => {
                ev.prevent_default();
                set_insert_menu_open.set(false);
                set_insert_filter.set(String::new());
                set_at_trigger_pos.set(None);
            }
            _ => {}
        }
    };

    // Handle input to detect '@' trigger and filter text
    let on_input = move |ev: ev::Event| {
        let target = ev.target().unwrap();
        let el: web_sys::HtmlTextAreaElement = target.unchecked_into();
        let val = el.value();
        let cursor = el.selection_start().unwrap_or(None).unwrap_or(0) as usize;

        set_content.set(val.clone());
        set_is_dirty.set(true);

        if insert_menu_open.get() {
            // Menu is already open — update filter based on text after '@'
            if let Some(trigger) = at_trigger_pos.get() {
                if cursor > trigger {
                    let filter = &val[trigger + 1..cursor];
                    // Close if a space is typed in filter
                    if filter.contains(' ') || filter.contains('\n') {
                        set_insert_menu_open.set(false);
                        set_insert_filter.set(String::new());
                        set_at_trigger_pos.set(None);
                    } else {
                        set_insert_filter.set(filter.to_string());
                        set_selected_index.set(0);
                    }
                } else {
                    // Cursor moved before '@' — close menu
                    set_insert_menu_open.set(false);
                    set_insert_filter.set(String::new());
                    set_at_trigger_pos.set(None);
                }
            }
        } else if cursor > 0 {
            // Check if '@' was just typed
            let byte_pos = cursor - 1;
            if byte_pos < val.len() && val.as_bytes()[byte_pos] == b'@' {
                // Only trigger if at start of line or preceded by whitespace
                let preceded_by_space = byte_pos == 0
                    || val.as_bytes()[byte_pos - 1] == b' '
                    || val.as_bytes()[byte_pos - 1] == b'\n';
                if preceded_by_space {
                    set_at_trigger_pos.set(Some(byte_pos));
                    set_insert_filter.set(String::new());
                    set_insert_menu_open.set(true);
                    set_selected_index.set(0);
                }
            }
        }
    };

    view! {
        <div class="editor-container">
            <div class="editor-gutter">
                <span class="pilcrow">"¶"</span>
            </div>
            <div class="editor-wrapper">
                <textarea
                    class="editor-textarea"
                    placeholder="Type @ to insert"
                    prop:value=move || content.get()
                    on:input=on_input
                    on:keydown=on_keydown
                    node_ref=textarea_ref
                ></textarea>
                <InsertMenuPopup
                    is_open=insert_menu_open
                    items=filtered_items
                    selected_index=selected_index
                    on_select=move |item: InsertItem| {
                        do_insert(&item);
                    }
                    on_close=move || {
                        set_insert_menu_open.set(false);
                        set_insert_filter.set(String::new());
                        set_at_trigger_pos.set(None);
                        // Re-focus the textarea
                        if let Some(el) = textarea_ref.get() {
                            let el: web_sys::HtmlTextAreaElement = el.into();
                            let _ = el.focus();
                        }
                    }
                />
            </div>
        </div>
    }
}

// ── InsertMenuPopup ──

/// Popup menu showing filterable markdown syntax items, triggered by typing '@'.
#[component]
fn InsertMenuPopup(
    is_open: ReadSignal<bool>,
    items: Memo<Vec<InsertItem>>,
    selected_index: ReadSignal<usize>,
    on_select: impl Fn(InsertItem) + 'static + Copy + Send + Sync,
    on_close: impl Fn() + 'static + Copy + Send + Sync,
) -> impl IntoView {
    view! {
        <Show when=move || is_open.get()>
            <div class="insert-menu-backdrop" on:click=move |_| on_close()></div>
            <div class="insert-menu">
                <div class="insert-menu-header">
                    <span class="insert-menu-title">"Insert block"</span>
                    <span class="insert-menu-hint">"Filter by typing"</span>
                </div>
                <div class="insert-menu-list">
                    {move || {
                        let current_items = items.get();
                        if current_items.is_empty() {
                            vec![
                                view! {
                                    <div class="insert-menu-empty">"No matches"</div>
                                }.into_any()
                            ]
                        } else {
                            current_items
                                .iter()
                                .enumerate()
                                .map(|(idx, item)| {
                                    let item_clone = item.clone();
                                    let is_selected = move || selected_index.get() == idx;
                                    let icon = item.icon;
                                    let label = item.label;
                                    let desc = item.description;
                                    view! {
                                        <button
                                            class="insert-menu-item"
                                            class:selected=is_selected
                                            on:mousedown=move |ev| {
                                                ev.prevent_default();
                                                on_select(item_clone.clone());
                                            }
                                        >
                                            <span class="insert-menu-item-icon">{icon}</span>
                                            <div class="insert-menu-item-text">
                                                <span class="insert-menu-item-label">{label}</span>
                                                <span class="insert-menu-item-desc">{desc}</span>
                                            </div>
                                        </button>
                                    }.into_any()
                                })
                                .collect::<Vec<_>>()
                        }
                    }}
                </div>
            </div>
        </Show>
    }
}
