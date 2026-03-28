pub mod wysiwym;

use leptos::ev;
use leptos::prelude::*;
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

// ── AI types ──

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ServerType {
    Openai,
    Ollama,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct AiServer {
    server_type: ServerType,
    name: String,
    api_base: String,
    api_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct AiSettings {
    servers: Vec<AiServer>,
    active_index: Option<usize>,
    last_model: Option<String>,
}

impl Default for AiSettings {
    fn default() -> Self {
        Self {
            servers: vec![],
            active_index: None,
            last_model: None,
        }
    }
}

impl AiSettings {
    fn active_server(&self) -> Option<&AiServer> {
        self.active_index.and_then(|i| self.servers.get(i))
    }

    fn has_active_connection(&self) -> bool {
        self.active_server().is_some()
    }
}

// Chat message (API-facing, for serializing to backend)
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    role: String,
    content: String,
}

// UI chat message — supports multiple response variants for assistant retries
#[derive(Debug, Clone)]
struct UiChatMessage {
    role: String,
    /// All response variants (for user messages this is always a single entry)
    variants: Vec<String>,
    /// Which variant is currently displayed
    active_variant: usize,
}

impl UiChatMessage {
    fn user(content: String) -> Self {
        Self {
            role: "user".to_string(),
            variants: vec![content],
            active_variant: 0,
        }
    }

    fn assistant(content: String) -> Self {
        Self {
            role: "assistant".to_string(),
            variants: vec![content],
            active_variant: 0,
        }
    }

    fn active_content(&self) -> &str {
        &self.variants[self.active_variant]
    }

    /// Convert to API ChatMessage using the active variant
    fn to_api(&self) -> ChatMessage {
        ChatMessage {
            role: self.role.clone(),
            content: self.active_content().to_string(),
        }
    }
}

// Chat response from backend
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatResponse {
    message: String,
    document_edit: Option<String>,
    diff: Option<String>,
}

// ── Tauri command args (camelCase for Tauri v2) ──

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveAiSettingsArgs {
    settings: AiSettings,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AiChatArgs {
    messages: Vec<ChatMessage>,
    document_content: String,
    edit_mode: bool,
    api_base: String,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AiAmendArgs {
    selected_text: String,
    instruction: String,
    api_base: String,
    api_key: String,
    model: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ListModelsArgs {
    api_base: String,
    api_key: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VerifyConnectionArgs {
    api_base: String,
    api_key: String,
}

// ── Helper ──

fn filename_from_path(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

// ── Markdown insert items ──

struct InsertItem {
    label: &'static str,
    icon: &'static str,
    desc: &'static str,
    snippet: &'static str,
}

const INSERT_ITEMS: &[InsertItem] = &[
    InsertItem {
        label: "Heading 1",
        icon: "H1",
        desc: "Large heading",
        snippet: "# ",
    },
    InsertItem {
        label: "Heading 2",
        icon: "H2",
        desc: "Medium heading",
        snippet: "## ",
    },
    InsertItem {
        label: "Heading 3",
        icon: "H3",
        desc: "Small heading",
        snippet: "### ",
    },
    InsertItem {
        label: "Bold",
        icon: "B",
        desc: "Strong text",
        snippet: "**bold**",
    },
    InsertItem {
        label: "Italic",
        icon: "I",
        desc: "Emphasized text",
        snippet: "*italic*",
    },
    InsertItem {
        label: "Link",
        icon: "L",
        desc: "Hyperlink",
        snippet: "[text](url)",
    },
    InsertItem {
        label: "Image",
        icon: "Img",
        desc: "Embedded image",
        snippet: "![alt](url)",
    },
    InsertItem {
        label: "Code Block",
        icon: "</>",
        desc: "Multiline code",
        snippet: "```\ncode\n```",
    },
    InsertItem {
        label: "Inline Code",
        icon: "`",
        desc: "Inline code snippet",
        snippet: "`code`",
    },
    InsertItem {
        label: "Blockquote",
        icon: "\"",
        desc: "Quoted text",
        snippet: "> ",
    },
    InsertItem {
        label: "Bullet List",
        icon: "•",
        desc: "Unordered list",
        snippet: "- item\n- item\n- item",
    },
    InsertItem {
        label: "Numbered List",
        icon: "1.",
        desc: "Ordered list",
        snippet: "1. item\n2. item\n3. item",
    },
    InsertItem {
        label: "Table",
        icon: "Tbl",
        desc: "Markdown table",
        snippet: "| Col 1 | Col 2 |\n|-------|-------|\n| A     | B     |",
    },
    InsertItem {
        label: "HR",
        icon: "---",
        desc: "Horizontal rule",
        snippet: "---",
    },
    InsertItem {
        label: "Task List",
        icon: "☑",
        desc: "Checkboxes",
        snippet: "- [ ] task\n- [x] done",
    },
];

// ── App component ──

#[component]
pub fn App() -> impl IntoView {
    // Core state
    let (content, set_content) = signal(String::new());
    let (file_path, set_file_path) = signal::<Option<String>>(None);
    let (is_dirty, set_is_dirty) = signal(false);
    let (is_dark, set_is_dark) = signal(true);
    let (menu_open, set_menu_open) = signal(false);
    let (settings_open, set_settings_open) = signal(false);
    let (chat_panel_open, set_chat_panel_open) = signal(false);

    // Counter mode: 0=chars, 1=words, 2=paragraphs
    let (counter_mode, set_counter_mode) = signal(0u8);

    // Edit mode toggle for chat panel
    let (edit_mode, set_edit_mode) = signal(true);

    // AI settings
    let (ai_settings, set_ai_settings) = signal(AiSettings::default());

    // Model selection — shared between chat panel and amend
    let (selected_model, set_selected_model) = signal(String::new());

    // Right-click context menu state
    let (ctx_menu, set_ctx_menu) = signal::<Option<ContextMenuState>>(None);

    // Amend dialog state
    let (amend_state, set_amend_state) = signal::<Option<AmendState>>(None);

    // Chat history
    let (chat_messages, set_chat_messages) = signal::<Vec<UiChatMessage>>(vec![]);

    // Load settings on mount
    Effect::new(move |_| {
        leptos::task::spawn_local(async move {
            let result: Result<AiSettings, _> = invoke("load_ai_settings", &EmptyArgs {}).await;
            if let Ok(settings) = result {
                // Restore last model
                if let Some(ref last) = settings.last_model {
                    set_selected_model.set(last.clone());
                }
                set_ai_settings.set(settings);
            }
        });
    });

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

    // File operations
    let do_open = move || {
        leptos::task::spawn_local(async move {
            let result: Option<OpenFileResponse> =
                invoke("open_file", &EmptyArgs {}).await.unwrap_or(None);
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
            let result: Option<String> = invoke("save_file_as", &SaveFileAsArgs { content: text })
                .await
                .unwrap_or(None);
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
                    .unwrap_or(());
                    set_is_dirty.set(false);
                });
            }
            None => {
                do_save_as();
            }
        }
    };

    // Window controls
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

    // Theme
    Effect::new(move |_| {
        let doc = web_sys::window().unwrap().document().unwrap();
        let body = doc.body().unwrap();
        let theme = if is_dark.get() { "dark" } else { "light" };
        body.set_attribute("data-theme", theme).unwrap();
    });

    view! {
        <div
            class="app-container"
            on:keydown=move |ev: ev::KeyboardEvent| {
                if (ev.ctrl_key() || ev.meta_key()) && (ev.key() == "s" || ev.key() == "S") {
                    ev.prevent_default();
                    do_save();
                }
            }
        >
            <TitleBar
                display_name=display_name
                content=content
                is_dirty=is_dirty
                counter_mode=counter_mode
                set_counter_mode=set_counter_mode
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
                set_settings_open=set_settings_open
                set_chat_panel_open=set_chat_panel_open
            />
            <div class="main-area">
                <Editor
                    content=content
                    set_content=set_content
                    set_is_dirty=set_is_dirty
                    ai_settings=ai_settings
                    _ctx_menu=ctx_menu
                    set_ctx_menu=set_ctx_menu
                />
                <ChatPanel
                    is_open=chat_panel_open
                    set_is_open=set_chat_panel_open
                    content=content
                    set_content=set_content
                    set_is_dirty=set_is_dirty
                    ai_settings=ai_settings
                    set_ai_settings=set_ai_settings
                    selected_model=selected_model
                    set_selected_model=set_selected_model
                    chat_messages=chat_messages
                    set_chat_messages=set_chat_messages
                    edit_mode=edit_mode
                    set_edit_mode=set_edit_mode
                />
            </div>
            <SettingsDialog
                is_open=settings_open
                set_is_open=set_settings_open
                ai_settings=ai_settings
                set_ai_settings=set_ai_settings
            />
            <ContextMenu
                ctx_menu=ctx_menu
                set_ctx_menu=set_ctx_menu
                set_amend_state=set_amend_state
                ai_settings=ai_settings
            />
            <AmendDialog
                amend_state=amend_state
                set_amend_state=set_amend_state
                content=content
                set_content=set_content
                set_is_dirty=set_is_dirty
                ai_settings=ai_settings
                selected_model=selected_model
            />
        </div>
    }
}

// ── Right-click context menu state ──

#[derive(Clone, PartialEq)]
struct ContextMenuState {
    x: i32,
    y: i32,
    sel_start: usize,
    sel_end: usize,
}

// ── Amend dialog state ──

#[derive(Clone, PartialEq)]
struct AmendState {
    sel_start: usize,
    sel_end: usize,
    selected_text: String,
}

// ── TitleBar ──

#[component]
fn TitleBar(
    display_name: Memo<String>,
    content: ReadSignal<String>,
    is_dirty: ReadSignal<bool>,
    counter_mode: ReadSignal<u8>,
    set_counter_mode: WriteSignal<u8>,
    menu_open: ReadSignal<bool>,
    set_menu_open: WriteSignal<bool>,
    on_minimize: impl Fn() + 'static + Copy + Send + Sync,
    on_maximize: impl Fn() + 'static + Copy + Send + Sync,
    on_close: impl Fn() + 'static + Copy + Send + Sync,
) -> impl IntoView {
    let counter_display = Memo::new(move |_| {
        let text = content.get();
        match counter_mode.get() {
            0 => format!("C {}", text.len()),
            1 => {
                let words = text.split_whitespace().count();
                format!("W {}", words)
            }
            _ => {
                let paras = text.split("\n\n").filter(|p| !p.trim().is_empty()).count();
                format!("P {}", paras)
            }
        }
    });

    view! {
        <div class="title-bar" data-tauri-drag-region="true">
            <div class="title-bar-left">
                <button class="title-bar-btn hamburger-btn" on:click=move |_| set_menu_open.set(!menu_open.get())>"☰"</button>
                <button class="title-bar-btn char-count" on:click=move |_| set_counter_mode.set((counter_mode.get() + 1) % 3)
                    title="Click to cycle: chars → words → paragraphs"
                >
                    {move || counter_display.get()}
                </button>
            </div>
            <div class="title-bar-center">
                <span class="file-name" class:dirty=move || is_dirty.get()>{move || display_name.get()}</span>
            </div>
            <div class="title-bar-right">
                <button class="title-bar-btn window-btn" on:click=move |_| on_minimize()>"─"</button>
                <button class="title-bar-btn window-btn" on:click=move |_| on_maximize()>"□"</button>
                <button class="title-bar-btn window-btn close-btn" on:click=move |_| on_close()>"✕"</button>
            </div>
        </div>
    }
}

// ── Menu Overlay ──

#[component]
fn MenuOverlay(
    is_open: ReadSignal<bool>,
    set_is_open: WriteSignal<bool>,
    is_dark: ReadSignal<bool>,
    set_is_dark: WriteSignal<bool>,
    on_open: impl Fn() + 'static + Copy + Send + Sync,
    on_save: impl Fn() + 'static + Copy + Send + Sync,
    on_save_as: impl Fn() + 'static + Copy + Send + Sync,
    set_settings_open: WriteSignal<bool>,
    set_chat_panel_open: WriteSignal<bool>,
) -> impl IntoView {
    view! {
        <Show when=move || is_open.get()>
            <div class="menu-backdrop" on:click=move |_| set_is_open.set(false)></div>
            <div class="menu-panel">
                <button class="menu-item" on:click=move |_| { on_open(); set_is_open.set(false); }>"📂 Open"</button>
                <button class="menu-item" on:click=move |_| { on_save(); set_is_open.set(false); }>"💾 Save"</button>
                <button class="menu-item" on:click=move |_| { on_save_as(); set_is_open.set(false); }>"📁 Save As"</button>
                <div class="menu-separator"></div>
                <button class="menu-item" on:click=move |_| { set_chat_panel_open.update(|v| *v = !*v); set_is_open.set(false); }>"🤖 AI Chat"</button>
                <button class="menu-item" on:click=move |_| { set_settings_open.set(true); set_is_open.set(false); }>"⚙️ Settings"</button>
                <div class="menu-separator"></div>
                <button class="menu-item" on:click=move |_| set_is_dark.set(!is_dark.get())>
                    {move || if is_dark.get() { "☀️ Light Theme" } else { "🌙 Dark Theme" }}
                </button>
                <div class="menu-separator"></div>
                <div class="menu-help">
                    <div class="menu-help-title">"Keybinds"</div>
                    <div class="menu-help-item"><span>"Ctrl/Cmd+S"</span><span>"Save"</span></div>
                    <div class="menu-help-item"><span>"Ctrl/Cmd+Z"</span><span>"Undo"</span></div>
                    <div class="menu-help-item"><span>"Ctrl/Cmd+X"</span><span>"Redo"</span></div>
                    <div class="menu-help-item"><span>"Ctrl/Cmd+R"</span><span>"Toggle Raw Line"</span></div>
                    <div class="menu-help-item"><span>"Arrow Up/Down"</span><span>"Move Between Lines"</span></div>
                </div>
            </div>
        </Show>
    }
}

// ── Settings Dialog ──

#[component]
fn SettingsDialog(
    is_open: ReadSignal<bool>,
    set_is_open: WriteSignal<bool>,
    ai_settings: ReadSignal<AiSettings>,
    set_ai_settings: WriteSignal<AiSettings>,
) -> impl IntoView {
    let (local_servers, set_local_servers) = signal::<Vec<AiServer>>(vec![]);
    let (local_active, set_local_active) = signal::<Option<usize>>(None);
    let (editing_idx, set_editing_idx) = signal::<Option<usize>>(None);
    let (edit_type, set_edit_type) = signal(ServerType::Openai);
    let (edit_name, set_edit_name) = signal(String::new());
    let (edit_base, set_edit_base) = signal(String::new());
    let (edit_key, set_edit_key) = signal(String::new());
    let (saving, set_saving) = signal(false);
    let (verify_status, set_verify_status) = signal::<Option<Result<(), String>>>(None);
    let (verifying, set_verifying) = signal(false);

    // Sync when dialog opens
    Effect::new(move |_| {
        if is_open.get() {
            let s = ai_settings.get();
            set_local_servers.set(s.servers);
            set_local_active.set(s.active_index);
            set_editing_idx.set(None);
        }
    });

    let start_add_openai = move || {
        set_edit_type.set(ServerType::Openai);
        set_edit_name.set(String::new());
        set_edit_base.set("https://api.openai.com/v1".to_string());
        set_edit_key.set(String::new());
        set_verify_status.set(None);
        set_editing_idx.set(Some(usize::MAX));
    };

    let start_add_ollama = move || {
        set_edit_type.set(ServerType::Ollama);
        set_edit_name.set(String::new());
        set_edit_base.set("http://localhost:11434/v1".to_string());
        set_edit_key.set(String::new());
        set_verify_status.set(None);
        set_editing_idx.set(Some(usize::MAX));
    };

    let start_edit = move |idx: usize| {
        let servers = local_servers.get();
        if let Some(s) = servers.get(idx) {
            set_edit_type.set(s.server_type.clone());
            set_edit_name.set(s.name.clone());
            set_edit_base.set(s.api_base.clone());
            set_edit_key.set(s.api_key.clone());
            set_verify_status.set(None);
            set_editing_idx.set(Some(idx));
        }
    };

    let do_verify = move || {
        let base = edit_base.get();
        let key = edit_key.get();
        set_verifying.set(true);
        set_verify_status.set(None);
        leptos::task::spawn_local(async move {
            let result: Result<bool, _> = invoke(
                "verify_connection",
                &VerifyConnectionArgs {
                    api_base: base,
                    api_key: key,
                },
            )
            .await;
            match result {
                Ok(_) => set_verify_status.set(Some(Ok(()))),
                Err(e) => set_verify_status.set(Some(Err(format!("{}", e)))),
            }
            set_verifying.set(false);
        });
    };

    let save_edit = move || {
        let server = AiServer {
            server_type: edit_type.get(),
            name: edit_name.get(),
            api_base: edit_base.get(),
            api_key: edit_key.get(),
        };
        let mut servers = local_servers.get();
        match editing_idx.get() {
            Some(idx) if idx == usize::MAX => {
                servers.push(server);
                if local_active.get().is_none() {
                    set_local_active.set(Some(servers.len() - 1));
                }
            }
            Some(idx) if idx < servers.len() => {
                servers[idx] = server;
            }
            _ => {}
        }
        set_local_servers.set(servers);
        set_editing_idx.set(None);
    };

    let delete_server = move |idx: usize| {
        let mut servers = local_servers.get();
        if idx < servers.len() {
            servers.remove(idx);
            let active = local_active.get();
            if servers.is_empty() {
                set_local_active.set(None);
            } else if let Some(a) = active {
                if a == idx {
                    set_local_active.set(Some(0));
                } else if a > idx {
                    set_local_active.set(Some(a - 1));
                }
            }
            set_local_servers.set(servers);
            if editing_idx.get() == Some(idx) {
                set_editing_idx.set(None);
            }
        }
    };

    let do_save_all = move || {
        let settings = AiSettings {
            servers: local_servers.get(),
            active_index: local_active.get(),
            last_model: ai_settings.get().last_model,
        };
        set_saving.set(true);
        let settings_c = settings.clone();
        leptos::task::spawn_local(async move {
            let _: Result<(), _> = invoke(
                "save_ai_settings",
                &SaveAiSettingsArgs {
                    settings: settings_c.clone(),
                },
            )
            .await;
            set_ai_settings.set(settings_c);
            set_saving.set(false);
            set_is_open.set(false);
        });
    };

    view! {
        <Show when=move || is_open.get()>
            <div class="dialog-backdrop" on:click=move |_| set_is_open.set(false)></div>
            <div class="dialog-panel dialog-wide">
                <h2 class="dialog-title">"⚙️ AI Server Settings"</h2>

                // Server list
                <div class="server-list">
                    {move || {
                        let servers = local_servers.get();
                        let active = local_active.get();
                        if servers.is_empty() {
                            vec![view! {
                                <div class="server-empty">"No servers configured. Add one below."</div>
                            }.into_any()]
                        } else {
                            servers.iter().enumerate().map(|(idx, server)| {
                                let name = server.name.clone();
                                let base = server.api_base.clone();
                                let type_label = match server.server_type {
                                    ServerType::Openai => "OpenAI",
                                    ServerType::Ollama => "Ollama",
                                };
                                let is_active = active == Some(idx);
                                view! {
                                    <div class="server-item" class:active=is_active>
                                        <button class="server-radio" on:click=move |_| set_local_active.set(Some(idx))>
                                            {if is_active { "●" } else { "○" }}
                                        </button>
                                        <div class="server-info">
                                            <span class="server-name">{name} <span class="server-type-badge">{type_label}</span></span>
                                            <span class="server-model">{base}</span>
                                        </div>
                                        <div class="server-actions">
                                            <button class="server-action-btn" on:click=move |_| start_edit(idx) title="Edit">"✏️"</button>
                                            <button class="server-action-btn" on:click=move |_| delete_server(idx) title="Delete">"🗑️"</button>
                                        </div>
                                    </div>
                                }.into_any()
                            }).collect::<Vec<_>>()
                        }
                    }}
                </div>

                <div class="add-server-buttons">
                    <button class="dialog-btn dialog-btn-add" on:click=move |_| start_add_openai()>"+ Add OpenAI Server"</button>
                    <button class="dialog-btn dialog-btn-add" on:click=move |_| start_add_ollama()>"+ Add Ollama Server"</button>
                </div>

                // Edit form
                <Show when=move || editing_idx.get().is_some()>
                    <div class="server-edit-form">
                        <div class="dialog-field">
                            <label class="dialog-label">"Server Type"</label>
                            <div class="server-type-selector">
                                <button
                                    class="type-btn"
                                    class:active=move || edit_type.get() == ServerType::Openai
                                    on:click=move |_| {
                                        set_edit_type.set(ServerType::Openai);
                                        set_edit_base.set("https://api.openai.com/v1".to_string());
                                    }
                                >"OpenAI"</button>
                                <button
                                    class="type-btn"
                                    class:active=move || edit_type.get() == ServerType::Ollama
                                    on:click=move |_| {
                                        set_edit_type.set(ServerType::Ollama);
                                        set_edit_base.set("http://localhost:11434/v1".to_string());
                                    }
                                >"Ollama"</button>
                            </div>
                        </div>
                        <div class="dialog-field">
                            <label class="dialog-label">"Name"</label>
                            <input class="dialog-input" type="text" placeholder="My Server"
                                prop:value=move || edit_name.get()
                                on:input=move |ev| set_edit_name.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="dialog-field">
                            <label class="dialog-label">"API Base URL"</label>
                            <input class="dialog-input" type="text"
                                placeholder=move || if edit_type.get() == ServerType::Ollama { "http://localhost:11434/v1" } else { "https://api.openai.com/v1" }
                                prop:value=move || edit_base.get()
                                on:input=move |ev| set_edit_base.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="dialog-field">
                            <label class="dialog-label">
                                {move || if edit_type.get() == ServerType::Ollama { "API Key (optional)" } else { "API Key" }}
                            </label>
                            <input class="dialog-input" type="password"
                                placeholder=move || if edit_type.get() == ServerType::Ollama { "Leave empty for local Ollama" } else { "sk-..." }
                                prop:value=move || edit_key.get()
                                on:input=move |ev| set_edit_key.set(event_target_value(&ev))
                            />
                        </div>
                        <div class="dialog-actions">
                            <button class="dialog-btn dialog-btn-verify"
                                on:click=move |_| do_verify()
                                disabled=move || verifying.get() || edit_base.get().trim().is_empty()
                            >
                                {move || if verifying.get() { "Verifying..." } else { "⚡ Verify Connection" }}
                            </button>
                        </div>
                        <Show when=move || verify_status.get().is_some()>
                            {move || match verify_status.get() {
                                Some(Ok(())) => view! { <div class="verify-success">"✅ Connection successful!"</div> }.into_any(),
                                Some(Err(e)) => view! { <div class="verify-error">"❌ " {e}</div> }.into_any(),
                                None => view! { <span></span> }.into_any(),
                            }}
                        </Show>
                        <div class="dialog-actions">
                            <button class="dialog-btn dialog-btn-secondary" on:click=move |_| set_editing_idx.set(None)>"Cancel"</button>
                            <button class="dialog-btn dialog-btn-primary" on:click=move |_| save_edit()>
                                {move || if editing_idx.get() == Some(usize::MAX) { "Add" } else { "Update" }}
                            </button>
                        </div>
                    </div>
                </Show>

                <div class="dialog-footer">
                    <button class="dialog-btn dialog-btn-secondary" on:click=move |_| set_is_open.set(false)>"Cancel"</button>
                    <button class="dialog-btn dialog-btn-primary" on:click=move |_| do_save_all() disabled=move || saving.get()>
                        {move || if saving.get() { "Saving..." } else { "Save All" }}
                    </button>
                </div>
            </div>
        </Show>
    }
}

// ── Context Menu (right-click on selected text) ──

#[component]
fn ContextMenu(
    ctx_menu: ReadSignal<Option<ContextMenuState>>,
    set_ctx_menu: WriteSignal<Option<ContextMenuState>>,
    set_amend_state: WriteSignal<Option<AmendState>>,
    ai_settings: ReadSignal<AiSettings>,
) -> impl IntoView {
    view! {
        <Show when=move || ctx_menu.get().is_some() && ai_settings.get().has_active_connection()>
            {move || {
                let state = ctx_menu.get().unwrap();
                let x = state.x;
                let y = state.y;
                let sel_start = state.sel_start;
                let sel_end = state.sel_end;

                view! {
                    <div class="ctx-backdrop" on:click=move |_| set_ctx_menu.set(None)></div>
                    <div class="ctx-menu" style=format!("left:{}px;top:{}px", x, y)>
                        <button class="ctx-item" on:click=move |_| {
                            // Get the selected text from the document
                            let doc = web_sys::window().unwrap().document().unwrap();
                            if let Some(textarea) = doc.query_selector("textarea.editor-source").ok().flatten() {
                                let ta: web_sys::HtmlTextAreaElement = textarea.unchecked_into();
                                let value = ta.value();
                                let selected = value[sel_start..sel_end].to_string();
                                set_amend_state.set(Some(AmendState {
                                    sel_start,
                                    sel_end,
                                    selected_text: selected,
                                }));
                            }
                            set_ctx_menu.set(None);
                        }>"✏️ Amend"</button>
                    </div>
                }
            }}
        </Show>
    }
}

// ── Amend Dialog ──

#[component]
fn AmendDialog(
    amend_state: ReadSignal<Option<AmendState>>,
    set_amend_state: WriteSignal<Option<AmendState>>,
    content: ReadSignal<String>,
    set_content: WriteSignal<String>,
    set_is_dirty: WriteSignal<bool>,
    ai_settings: ReadSignal<AiSettings>,
    selected_model: ReadSignal<String>,
) -> impl IntoView {
    let (instruction, set_instruction) = signal(String::new());
    let (loading, set_loading) = signal(false);
    let (error, set_error) = signal::<Option<String>>(None);

    let do_amend = move || {
        let state = match amend_state.get() {
            Some(s) => s,
            None => return,
        };
        let instr = instruction.get();
        if instr.trim().is_empty() {
            return;
        }
        let model = selected_model.get();
        if model.is_empty() {
            set_error.set(Some("Select a model in the LLM Panel first.".to_string()));
            return;
        }

        let settings = ai_settings.get();
        let server = match settings.active_server() {
            Some(s) => s.clone(),
            None => {
                set_error.set(Some("No active server configured.".to_string()));
                return;
            }
        };

        set_loading.set(true);
        set_error.set(None);

        let selected_text = state.selected_text.clone();
        let sel_start = state.sel_start;
        let sel_end = state.sel_end;

        leptos::task::spawn_local(async move {
            let result: Result<String, _> = invoke(
                "ai_amend",
                &AiAmendArgs {
                    selected_text,
                    instruction: instr,
                    api_base: server.api_base,
                    api_key: server.api_key,
                    model,
                },
            )
            .await;

            match result {
                Ok(amended) => {
                    // Replace selection in document
                    let doc = content.get();
                    let mut new_content = String::new();
                    new_content.push_str(&doc[..sel_start]);
                    new_content.push_str(&amended);
                    new_content.push_str(&doc[sel_end..]);
                    set_content.set(new_content);
                    set_is_dirty.set(true);
                    set_amend_state.set(None);
                    set_instruction.set(String::new());
                    set_error.set(None);
                }
                Err(e) => {
                    set_error.set(Some(format!("{}", e)));
                }
            }
            set_loading.set(false);
        });
    };

    view! {
        <Show when=move || amend_state.get().is_some()>
            <div class="dialog-backdrop" on:click=move |_| { set_amend_state.set(None); set_instruction.set(String::new()); }></div>
            <div class="dialog-panel amend-dialog">
                <h2 class="dialog-title">"✏️ Amend Selection"</h2>
                <div class="amend-preview">
                    <label class="dialog-label">"Selected Text"</label>
                    <pre class="amend-selected-text">{move || amend_state.get().map(|s| s.selected_text).unwrap_or_default()}</pre>
                </div>
                <div class="dialog-field">
                    <label class="dialog-label">"What changes should be made?"</label>
                    <textarea class="dialog-input amend-instruction"
                        placeholder="e.g. Fix grammar, make more concise, change tone to formal..."
                        prop:value=move || instruction.get()
                        on:input=move |ev| set_instruction.set(event_target_value(&ev))
                        on:keydown=move |ev: ev::KeyboardEvent| {
                            if ev.key() == "Enter" && (ev.ctrl_key() || ev.meta_key()) {
                                ev.prevent_default();
                                do_amend();
                            }
                        }
                    ></textarea>
                </div>
                <Show when=move || error.get().is_some()>
                    <div class="verify-error">{move || error.get().unwrap_or_default()}</div>
                </Show>
                <div class="dialog-actions">
                    <button class="dialog-btn dialog-btn-secondary"
                        on:click=move |_| { set_amend_state.set(None); set_instruction.set(String::new()); }
                    >"Cancel"</button>
                    <button class="dialog-btn dialog-btn-primary"
                        on:click=move |_| do_amend()
                        disabled=move || loading.get() || instruction.get().trim().is_empty()
                    >
                        {move || if loading.get() { "Amending..." } else { "Apply" }}
                    </button>
                </div>
                <Show when=move || loading.get()>
                    <div class="ai-loading">
                        <div class="ai-spinner"></div>
                        <span>"Processing..."</span>
                    </div>
                </Show>
            </div>
        </Show>
    }
}

// ── Chat Panel (LLM Panel) ──

#[component]
fn ChatPanel(
    is_open: ReadSignal<bool>,
    set_is_open: WriteSignal<bool>,
    content: ReadSignal<String>,
    set_content: WriteSignal<String>,
    set_is_dirty: WriteSignal<bool>,
    ai_settings: ReadSignal<AiSettings>,
    set_ai_settings: WriteSignal<AiSettings>,
    selected_model: ReadSignal<String>,
    set_selected_model: WriteSignal<String>,
    chat_messages: ReadSignal<Vec<UiChatMessage>>,
    set_chat_messages: WriteSignal<Vec<UiChatMessage>>,
    edit_mode: ReadSignal<bool>,
    set_edit_mode: WriteSignal<bool>,
) -> impl IntoView {
    let (input_text, set_input_text) = signal(String::new());
    let (loading, set_loading) = signal(false);

    // Model dropdown state
    let (available_models, set_available_models) = signal::<Vec<String>>(vec![]);
    let (model_filter, set_model_filter) = signal(String::new());
    let (model_dropdown_open, set_model_dropdown_open) = signal(false);
    let (models_loading, set_models_loading) = signal(false);

    let filtered_models = Memo::new(move |_| {
        let filter = model_filter.get().to_lowercase();
        let all = available_models.get();
        if filter.is_empty() {
            all
        } else {
            all.into_iter()
                .filter(|m| m.to_lowercase().contains(&filter))
                .collect()
        }
    });

    // Load models when panel opens
    Effect::new(move |_| {
        if is_open.get() {
            let settings = ai_settings.get();
            if let Some(server) = settings.active_server() {
                let base = server.api_base.clone();
                let key = server.api_key.clone();
                set_models_loading.set(true);
                leptos::task::spawn_local(async move {
                    let result: Result<Vec<String>, _> = invoke(
                        "list_models",
                        &ListModelsArgs {
                            api_base: base,
                            api_key: key,
                        },
                    )
                    .await;
                    if let Ok(models) = result {
                        set_available_models.set(models);
                    }
                    set_models_loading.set(false);
                });
            }
        }
    });

    // Persist model selection
    let save_model_choice = move |model: String| {
        set_selected_model.set(model.clone());
        let mut settings = ai_settings.get();
        settings.last_model = Some(model);
        let settings_c = settings.clone();
        set_ai_settings.set(settings.clone());
        leptos::task::spawn_local(async move {
            let _: Result<(), _> = invoke(
                "save_ai_settings",
                &SaveAiSettingsArgs {
                    settings: settings_c,
                },
            )
            .await;
        });
    };

    let do_send = move || {
        let msg = input_text.get();
        if msg.trim().is_empty() || loading.get() {
            return;
        }
        let model = selected_model.get();
        if model.is_empty() {
            return;
        }

        let settings = ai_settings.get();
        let server = match settings.active_server() {
            Some(s) => s.clone(),
            None => return,
        };

        // Add user message to history
        let user_msg = UiChatMessage::user(msg.clone());
        set_chat_messages.update(|msgs| msgs.push(user_msg));
        set_input_text.set(String::new());
        set_loading.set(true);

        let all_messages: Vec<ChatMessage> = chat_messages.get().iter().map(|m| m.to_api()).collect();
        let doc = content.get();
        let is_edit = edit_mode.get();

        leptos::task::spawn_local(async move {
            let result: Result<ChatResponse, _> = invoke(
                "ai_chat",
                &AiChatArgs {
                    messages: all_messages,
                    document_content: doc,
                    edit_mode: is_edit,
                    api_base: server.api_base,
                    api_key: server.api_key,
                    model,
                },
            )
            .await;

            match result {
                Ok(resp) => {
                    // Apply document edit if present
                    let had_edit = resp.document_edit.is_some();
                    if let Some(new_doc) = resp.document_edit {
                        set_content.set(new_doc);
                        set_is_dirty.set(true);
                    }
                    // Build assistant message — include diff if available
                    let mut msg_content = resp.message;
                    if had_edit {
                        if let Some(diff_text) = resp.diff {
                            if !diff_text.trim().is_empty() {
                                msg_content.push_str("\n\n📝 Changes:\n");
                                msg_content.push_str(&diff_text);
                            }
                        }
                    }
                    let assistant_msg = UiChatMessage::assistant(msg_content);
                    set_chat_messages.update(|msgs| msgs.push(assistant_msg));
                }
                Err(e) => {
                    let err_msg = UiChatMessage::assistant(format!("⚠️ Error: {}", e));
                    set_chat_messages.update(|msgs| msgs.push(err_msg));
                }
            }
            set_loading.set(false);

            // Auto-scroll chat to bottom after DOM updates
            {
                let doc = web_sys::window().unwrap().document().unwrap();
                if let Some(el) = doc.query_selector(".chat-messages").ok().flatten() {
                    el.set_scroll_top(el.scroll_height());
                }
            }
        });
    };

    let clear_chat = move || {
        set_chat_messages.set(vec![]);
    };

    // Retry: re-send the user message that's just before the given assistant message index
    let do_retry = move |assistant_idx: usize| {
        if loading.get() {
            return;
        }
        let model = selected_model.get();
        if model.is_empty() {
            return;
        }
        let settings = ai_settings.get();
        let server = match settings.active_server() {
            Some(s) => s.clone(),
            None => return,
        };

        // Gather API messages up to (but NOT including) the assistant message at assistant_idx
        let api_messages: Vec<ChatMessage> = chat_messages
            .get()
            .iter()
            .take(assistant_idx)
            .map(|m| m.to_api())
            .collect();

        let doc = content.get();
        let is_edit = edit_mode.get();
        set_loading.set(true);

        leptos::task::spawn_local(async move {
            let result: Result<ChatResponse, _> = invoke(
                "ai_chat",
                &AiChatArgs {
                    messages: api_messages,
                    document_content: doc,
                    edit_mode: is_edit,
                    api_base: server.api_base,
                    api_key: server.api_key,
                    model,
                },
            )
            .await;

            match result {
                Ok(resp) => {
                    let had_edit = resp.document_edit.is_some();
                    if let Some(new_doc) = resp.document_edit {
                        set_content.set(new_doc);
                        set_is_dirty.set(true);
                    }
                    let mut msg_content = resp.message;
                    if had_edit {
                        if let Some(diff_text) = resp.diff {
                            if !diff_text.trim().is_empty() {
                                msg_content.push_str("\n\n📝 Changes:\n");
                                msg_content.push_str(&diff_text);
                            }
                        }
                    }
                    // Append new variant to the existing assistant message
                    set_chat_messages.update(|msgs| {
                        if let Some(msg) = msgs.get_mut(assistant_idx) {
                            msg.variants.push(msg_content);
                            msg.active_variant = msg.variants.len() - 1;
                        }
                    });
                }
                Err(e) => {
                    // Append error as another variant
                    set_chat_messages.update(|msgs| {
                        if let Some(msg) = msgs.get_mut(assistant_idx) {
                            msg.variants.push(format!("⚠️ Error: {}", e));
                            msg.active_variant = msg.variants.len() - 1;
                        }
                    });
                }
            }
            set_loading.set(false);

            // Auto-scroll
            {
                let doc = web_sys::window().unwrap().document().unwrap();
                if let Some(el) = doc.query_selector(".chat-messages").ok().flatten() {
                    el.set_scroll_top(el.scroll_height());
                }
            }
        });
    };

    let mode_label = move || {
        if edit_mode.get() {
            "Edit mode"
        } else {
            "Chat mode"
        }
    };

    view! {
        <Show when=move || is_open.get()>
            <div class="chat-panel">
                <div class="chat-panel-header">
                    <div class="chat-panel-heading">
                        <span class="chat-panel-title">"AI Chat"</span>
                        <span class="chat-panel-subtitle">
                            {mode_label}
                            " "
                            {move || if edit_mode.get() { "lets the assistant apply document changes." } else { "keeps the assistant discussion-only." }}
                        </span>
                    </div>
                    <div class="chat-header-actions">
                        <div class="chat-mode-switch" role="tablist" aria-label="AI mode">
                            <button
                                class="chat-mode-btn"
                                class:active=move || !edit_mode.get()
                                on:click=move |_| set_edit_mode.set(false)
                                title="Chat mode — AI will only discuss, not edit"
                            >
                                "Chat"
                            </button>
                            <button
                                class="chat-mode-btn"
                                class:active=move || edit_mode.get()
                                on:click=move |_| set_edit_mode.set(true)
                                title="Edit mode — AI can modify your document"
                            >
                                "Edit"
                            </button>
                        </div>
                        <button class="chat-header-btn" on:click=move |_| clear_chat() title="Clear chat">"🗑️"</button>
                        <button class="chat-header-btn" on:click=move |_| set_is_open.set(false) title="Close">"✕"</button>
                    </div>
                </div>

                // Model selector
                <div class="model-selector">
                    <label class="model-label">"Model"</label>
                    <div class="model-dropdown-wrap">
                        <input
                            class="model-dropdown-input"
                            type="text"
                            placeholder=move || if models_loading.get() { "Loading models..." } else { "Search models..." }
                            prop:value=move || {
                                if model_dropdown_open.get() { model_filter.get() }
                                else { selected_model.get() }
                            }
                            on:focus=move |_| {
                                set_model_dropdown_open.set(true);
                                set_model_filter.set(String::new());
                            }
                            on:input=move |ev| {
                                set_model_filter.set(event_target_value(&ev));
                                set_model_dropdown_open.set(true);
                            }
                            on:keydown=move |ev: ev::KeyboardEvent| {
                                if ev.key() == "Escape" { set_model_dropdown_open.set(false); }
                            }
                        />
                        <Show when=move || model_dropdown_open.get()>
                            <div class="model-dropdown-backdrop" on:click=move |_| set_model_dropdown_open.set(false)></div>
                            <div class="model-dropdown-list">
                                {move || {
                                    let models = filtered_models.get();
                                    if models.is_empty() {
                                        vec![view! { <div class="model-dropdown-empty">"No models found"</div> }.into_any()]
                                    } else {
                                        models.iter().map(|m| {
                                            let model_id = m.clone();
                                            let model_id2 = m.clone();
                                            let is_selected = selected_model.get() == *m;
                                            view! {
                                                <button
                                                    class="model-dropdown-item"
                                                    class:selected=is_selected
                                                    on:mousedown=move |ev| {
                                                        ev.prevent_default();
                                                        save_model_choice(model_id.clone());
                                                        set_model_dropdown_open.set(false);
                                                    }
                                                >
                                                    {model_id2}
                                                </button>
                                            }.into_any()
                                        }).collect::<Vec<_>>()
                                    }
                                }}
                            </div>
                        </Show>
                    </div>
                </div>

                // Chat messages
                <div class="chat-messages">
                    {move || {
                        let msgs = chat_messages.get();
                        if msgs.is_empty() {
                            vec![view! {
                                <div class="chat-empty">
                                    <div class="chat-empty-icon">"💬"</div>
                                    <div class="chat-empty-text">"Start a conversation to get help with your document"</div>
                                </div>
                            }.into_any()]
                        } else {
                            msgs.iter().enumerate().map(|(msg_idx, msg)| {
                                let role = msg.role.clone();
                                let content_text = msg.active_content().to_string();
                                let is_user = role == "user";
                                let variant_count = msg.variants.len();
                                let active_variant = msg.active_variant;

                                // For assistant messages, split out diff section
                                let (text_part, diff_part) = if !is_user {
                                    if let Some(idx) = content_text.find("\n\n📝 Changes:\n") {
                                        let text = content_text[..idx].to_string();
                                        let diff = content_text[idx + "\n\n📝 Changes:\n".len()..].to_string();
                                        (text, Some(diff))
                                    } else {
                                        (content_text.clone(), None)
                                    }
                                } else {
                                    (content_text.clone(), None)
                                };

                                let has_diff = diff_part.is_some();
                                let diff_text = diff_part.unwrap_or_default();
                                let show_controls = !is_user;
                                let has_variants = variant_count > 1;
                                let is_last_variant = active_variant == variant_count - 1;

                                view! {
                                    <div class="chat-bubble" class:user=is_user class:assistant=!is_user>
                                        <div class="chat-role">{if is_user { "You" } else { "AI" }}</div>
                                        <div class="chat-content">
                                            <pre class="chat-text">{text_part}</pre>
                                            <Show when=move || has_diff>
                                                <div class="chat-diff-label">"📝 Changes"</div>
                                                <pre class="chat-diff">{diff_text.clone()}</pre>
                                            </Show>
                                        </div>
                                        <Show when=move || show_controls>
                                            <div class="chat-bubble-actions">
                                                <Show when=move || has_variants>
                                                    <div class="chat-variant-nav">
                                                        <button
                                                            class="chat-variant-btn"
                                                            disabled=move || active_variant == 0
                                                            on:click=move |_| {
                                                                set_chat_messages.update(|msgs| {
                                                                    if let Some(m) = msgs.get_mut(msg_idx) {
                                                                        if m.active_variant > 0 {
                                                                            m.active_variant -= 1;
                                                                        }
                                                                    }
                                                                });
                                                            }
                                                        >"‹"</button>
                                                        <span class="chat-variant-counter">
                                                            {format!("{}/{}", active_variant + 1, variant_count)}
                                                        </span>
                                                        <button
                                                            class="chat-variant-btn"
                                                            disabled=move || is_last_variant
                                                            on:click=move |_| {
                                                                set_chat_messages.update(|msgs| {
                                                                    if let Some(m) = msgs.get_mut(msg_idx) {
                                                                        if m.active_variant < m.variants.len() - 1 {
                                                                            m.active_variant += 1;
                                                                        }
                                                                    }
                                                                });
                                                            }
                                                        >"›"</button>
                                                    </div>
                                                </Show>
                                                <button
                                                    class="chat-retry-btn"
                                                    title="Retry this response"
                                                    disabled=move || loading.get()
                                                    on:click=move |_| do_retry(msg_idx)
                                                >"↻"</button>
                                            </div>
                                        </Show>
                                    </div>
                                }.into_any()
                            }).collect::<Vec<_>>()
                        }
                    }}
                    <Show when=move || loading.get()>
                        <div class="chat-bubble assistant">
                            <div class="chat-role">"AI"</div>
                            <div class="chat-content">
                                <div class="chat-typing">
                                    <span class="typing-dot"></span>
                                    <span class="typing-dot"></span>
                                    <span class="typing-dot"></span>
                                </div>
                            </div>
                        </div>
                    </Show>
                </div>

                // Input area
                <div class="chat-input-area">
                    <textarea
                        class="chat-input"
                        placeholder=move || {
                            if selected_model.get().is_empty() { "Select a model first..." }
                            else { "Ask about your document..." }
                        }
                        prop:value=move || input_text.get()
                        on:input=move |ev| set_input_text.set(event_target_value(&ev))
                        on:keydown=move |ev: ev::KeyboardEvent| {
                            if ev.key() == "Enter" && !ev.shift_key() {
                                ev.prevent_default();
                                do_send();
                            }
                        }
                        disabled=move || selected_model.get().is_empty()
                    ></textarea>
                    <button class="chat-send-btn"
                        on:click=move |_| do_send()
                        disabled=move || loading.get() || input_text.get().trim().is_empty() || selected_model.get().is_empty()
                    >"↑"</button>
                </div>
            </div>
        </Show>
    }
}

// ── Editor with @ Insert Menu + right-click ──

#[component]
fn InsertMenuPopup(
    insert_menu: ReadSignal<Option<(i32, i32, usize)>>,
    set_insert_menu: WriteSignal<Option<(i32, i32, usize)>>,
    filter_text: ReadSignal<String>,
    #[prop(into)] insert_item: Callback<&'static str, ()>,
) -> impl IntoView {
    let filtered_items = move || {
        let query = filter_text.get().to_lowercase();
        INSERT_ITEMS
            .iter()
            .filter(|item| item.label.to_lowercase().contains(&query))
            .collect::<Vec<_>>()
    };

    view! {
        <Show when=move || insert_menu.get().is_some()>
            {move || {
                let (x, y, _) = insert_menu.get().unwrap();
                let style = format!("left: {}px; top: {}px;", x, y);

                view! {
                    <div class="insert-menu-backdrop" on:click=move |_| set_insert_menu.set(None)></div>
                    <div class="insert-menu" style=style>
                        <div class="insert-menu-header">
                            <span class="insert-menu-title">"Insert"</span>
                            <span class="insert-menu-hint">"Press Esc to close"</span>
                        </div>
                        <div class="insert-menu-list">
                            <For
                                each=filtered_items
                                key=|item| item.label
                                children={move |item| {
                                    let snippet = item.snippet;
                                    view! {
                                        <button class="insert-menu-item" on:click=move |_| insert_item.run(snippet)>
                                            <div class="insert-menu-item-icon">{item.icon.to_string()}</div>
                                            <div class="insert-menu-item-text">
                                                <div class="insert-menu-item-label">{item.label.to_string()}</div>
                                                <div class="insert-menu-item-desc">{item.desc.to_string()}</div>
                                            </div>
                                        </button>
                                    }
                                }}
                            />
                        </div>
                    </div>
                }
            }}
        </Show>
    }
}

#[component]
fn Editor(
    content: ReadSignal<String>,
    set_content: WriteSignal<String>,
    set_is_dirty: WriteSignal<bool>,
    ai_settings: ReadSignal<AiSettings>,
    _ctx_menu: ReadSignal<Option<ContextMenuState>>,
    set_ctx_menu: WriteSignal<Option<ContextMenuState>>,
) -> impl IntoView {
    let undo_stack = RwSignal::new(Vec::<String>::new());
    let redo_stack = RwSignal::new(Vec::<String>::new());
    undo_stack.update(|s| s.push(content.get_untracked()));
    let editor_state = wysiwym::EditorState::new(&content.get_untracked());
    let (insert_menu, set_insert_menu) = signal::<Option<(i32, i32, usize)>>(None);
    let (filter_text, set_filter_text) = signal(String::new());

    Effect::new(move |_| {
        let current_text = content.get();
        if current_text != editor_state.to_string() {
            editor_state.sync_from_content(&current_text);
        }
    });

    let insert_item = move |snippet: &str| {
        if let Some((_, _, at_pos)) = insert_menu.get() {
            let val = content.get_untracked();
            if at_pos > 0 && at_pos <= val.len() {
                let before = &val[..at_pos - 1];
                let after = &val[at_pos..];
                let new_val = format!("{}{}{}", before, snippet, after);
                undo_stack.update(|s| s.push(val.clone()));
                redo_stack.update(|s| s.clear());
                editor_state.sync_from_content(&new_val);
                set_content.set(new_val);
                set_is_dirty.set(true);
            }
        }
        set_insert_menu.set(None);
    };

    let on_contextmenu = move |ev: ev::MouseEvent| {
        if ai_settings.get().has_active_connection() {
            set_ctx_menu.set(None);
        } else {
            ev.prevent_default();
        }
    };

    let on_block_change: Callback<(), ()> = Callback::new(move |_| {
        let new_text = editor_state.to_string();
        let old_text = content.get_untracked();
        if old_text != new_text {
            undo_stack.update(|s| s.push(old_text));
            redo_stack.update(|s| s.clear());
            set_content.set(new_text);
            set_is_dirty.set(true);
        }
    });

    let on_at_menu: Callback<(i32, i32, usize), ()> = Callback::new(move |(x, y, cursor)| {
        set_insert_menu.set(Some((x, y, cursor)));
        set_filter_text.set(String::new());
    });

    let on_keydown = move |ev: ev::KeyboardEvent| {
        if insert_menu.get().is_some() && ev.key() == "Escape" {
            set_insert_menu.set(None);
            ev.prevent_default();
            return;
        }
        if ev.ctrl_key() || ev.meta_key() {
            if ev.key() == "z" {
                ev.prevent_default();
                undo_stack.update(|s| {
                    if let Some(prev) = s.pop() {
                        redo_stack.update(|r| r.push(content.get_untracked()));
                        set_content.set(prev.clone());
                        editor_state.sync_from_content(&prev);
                        set_is_dirty.set(true);
                    }
                });
            } else if ev.key() == "x" || ev.key() == "X" {
                ev.prevent_default();
                redo_stack.update(|r| {
                    if let Some(next) = r.pop() {
                        undo_stack.update(|s| s.push(content.get_untracked()));
                        set_content.set(next.clone());
                        editor_state.sync_from_content(&next);
                        set_is_dirty.set(true);
                    }
                });
            }
        }
    };

    view! {
        <div class="editor-wrapper" on:keydown=on_keydown>
            <div class="editor-lines" on:contextmenu=on_contextmenu>
                <For
                    each=move || editor_state.blocks.get()
                    key=|block| block.id
                    children={move |block| {
                        view! {
                            <wysiwym::EditorBlockComponent
                                state=editor_state
                                block=block
                                on_change=on_block_change
                                on_at_menu=on_at_menu
                            />
                        }
                    }}
                />
            </div>
            <InsertMenuPopup
                insert_menu=insert_menu
                set_insert_menu=set_insert_menu
                filter_text=filter_text
                insert_item=insert_item
            />
        </div>
    }
}

// File ends here.
