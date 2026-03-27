#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use langchain_rust::{
    chain::{Chain, LLMChainBuilder},
    fmt_message, fmt_template,
    llm::openai::{OpenAI, OpenAIConfig},
    message_formatter,
    prompt::HumanMessagePromptTemplate,
    prompt_args,
    schemas::Message,
    template_fstring,
};
use std::fs;
use std::path::PathBuf;
use tauri_plugin_dialog::DialogExt;

// ── AI Settings (multi-server, TOML) ──

/// Server type for provider-specific defaults.
#[derive(Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
enum ServerType {
    Openai,
    Ollama,
}

/// A single OpenAI-compatible server configuration.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct AiServer {
    /// "openai" or "ollama"
    server_type: ServerType,
    /// User-friendly name (e.g. "OpenAI", "Local Ollama")
    name: String,
    /// OpenAI-compatible API base URL
    api_base: String,
    /// API key (optional for Ollama)
    api_key: String,
}

/// Persistent AI configuration.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct AiSettings {
    servers: Vec<AiServer>,
    active_index: Option<usize>,
    /// Last selected model, persisted across sessions
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

/// A single chat message exchanged between user and assistant.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ChatMessage {
    role: String, // "user" or "assistant"
    content: String,
}

#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct ChatResponse {
    message: String,
    /// If the AI wants to edit the document, this contains the new full content.
    document_edit: Option<String>,
    /// Unified git diff of the document edit
    diff: Option<String>,
}

fn settings_path() -> Result<PathBuf, String> {
    let config_dir =
        dirs::config_dir().ok_or_else(|| "Could not determine config directory".to_string())?;
    let app_dir = config_dir.join("WriterMD");
    fs::create_dir_all(&app_dir).map_err(|e| format!("Failed to create config dir: {}", e))?;
    Ok(app_dir.join("settings.toml"))
}

fn build_get_request(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
) -> reqwest::RequestBuilder {
    let req = client.get(url);
    if api_key.is_empty() {
        req
    } else {
        req.header("Authorization", format!("Bearer {}", api_key))
    }
}

fn build_openai(api_base: &str, api_key: &str, model: &str) -> OpenAI<OpenAIConfig> {
    let mut config = OpenAIConfig::new().with_api_base(api_base);
    if !api_key.is_empty() {
        config = config.with_api_key(api_key);
    }
    OpenAI::default().with_config(config).with_model(model)
}

// ── Tauri Commands ──

#[tauri::command]
async fn load_ai_settings() -> Result<AiSettings, String> {
    let path = settings_path()?;
    if path.exists() {
        let data =
            fs::read_to_string(&path).map_err(|e| format!("Failed to read settings: {}", e))?;
        toml::from_str(&data).map_err(|e| format!("Failed to parse settings: {}", e))
    } else {
        Ok(AiSettings::default())
    }
}

#[tauri::command]
async fn save_ai_settings(settings: AiSettings) -> Result<(), String> {
    let path = settings_path()?;
    let data = toml::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;
    fs::write(&path, data).map_err(|e| format!("Failed to write settings: {}", e))
}

#[tauri::command]
async fn verify_connection(api_base: String, api_key: String) -> Result<bool, String> {
    let url = format!("{}/models", api_base.trim_end_matches('/'));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("HTTP client error: {}", e))?;

    let resp = build_get_request(&client, &url, &api_key)
        .send()
        .await
        .map_err(|e| {
            if e.is_connect() {
                "Connection failed — check the URL and that the server is running".to_string()
            } else if e.is_timeout() {
                "Connection timed out".to_string()
            } else {
                format!("Request failed: {}", e)
            }
        })?;

    if resp.status().is_success() {
        Ok(true)
    } else if resp.status().as_u16() == 401 {
        Err("Authentication failed — check your API key".to_string())
    } else {
        Err(format!("Server returned status {}", resp.status()))
    }
}

#[tauri::command]
async fn list_models(api_base: String, api_key: String) -> Result<Vec<String>, String> {
    let url = format!("{}/models", api_base.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let resp = build_get_request(&client, &url, &api_key)
        .send()
        .await
        .map_err(|e| format!("Failed to fetch models: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("API returned status {}", resp.status()));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse models response: {}", e))?;

    let mut models: Vec<String> = body["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m["id"].as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    models.sort();
    Ok(models)
}

/// Chat with the AI assistant. Supports conversation history.
/// When edit_mode is true, the AI can edit the document via <DOCUMENT_EDIT> tags.
/// When false, the AI only chats without making changes.
#[tauri::command]
async fn ai_chat(
    messages: Vec<ChatMessage>,
    document_content: String,
    edit_mode: bool,
    api_base: String,
    api_key: String,
    model: String,
) -> Result<ChatResponse, String> {
    let llm = build_openai(&api_base, &api_key, &model);

    // Build conversation history as context
    let mut history = String::new();
    let recent: Vec<&ChatMessage> = messages
        .iter()
        .rev()
        .take(10)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    for msg in &recent[..recent.len().saturating_sub(1)] {
        history.push_str(&format!("[{}]: {}\n", msg.role, msg.content));
    }

    let user_message = recent.last().map(|m| m.content.clone()).unwrap_or_default();

    let edit_instructions = if edit_mode {
        "- When the user asks you to make changes to the document, you MUST include the full edited document \
           wrapped in <DOCUMENT_EDIT> tags. Example:\n\
           <DOCUMENT_EDIT>\n# New Title\nNew content here...\n</DOCUMENT_EDIT>\n\
         - You can include explanation text BEFORE or AFTER the edit tags.\n\
         - Only include <DOCUMENT_EDIT> tags when you are actually changing the document.\n\
         - Always produce well-formatted markdown inside the edit tags."
    } else {
        "- IMPORTANT: You are in CHAT-ONLY mode. Do NOT edit the document. \
         Do NOT include <DOCUMENT_EDIT> tags. Only discuss, advise, and answer questions about the document."
    };

    let system_prompt = format!(
        "You are an expert markdown writing assistant embedded in WriterMD, a markdown editor.\n\
         You help the user write, edit, and structure their documents.\n\n\
         CURRENT DOCUMENT:\n```markdown\n{}\n```\n\n\
         CONVERSATION HISTORY:\n{}\n\n\
         INSTRUCTIONS:\n\
         - Respond conversationally to help the user with their document.\n\
         {}",
        if document_content.is_empty() {
            "(empty document)"
        } else {
            &document_content
        },
        if history.is_empty() {
            "(new conversation)"
        } else {
            &history
        },
        edit_instructions
    );

    let prompt_template = message_formatter![
        fmt_message!(Message::new_system_message(&system_prompt)),
        fmt_template!(HumanMessagePromptTemplate::new(template_fstring!(
            "{input}", "input"
        )))
    ];

    let chain = LLMChainBuilder::new()
        .prompt(prompt_template)
        .llm(llm)
        .build()
        .map_err(|e| format!("Failed to build chain: {}", e))?;

    let result = chain
        .invoke(prompt_args! { "input" => user_message })
        .await
        .map_err(|e| format!("AI chat failed: {}", e))?;

    // Parse for document edits
    let (message, document_edit) = parse_document_edit(&result);
    let mut diff = None;

    if let Some(ref new_content) = document_edit {
        diff = Some(
            similar::TextDiff::from_lines(&document_content, new_content)
                .unified_diff()
                .context_radius(3)
                .to_string(),
        );
    }

    Ok(ChatResponse {
        message,
        document_edit,
        diff,
    })
}

/// Parse AI response for <DOCUMENT_EDIT> tags.
fn parse_document_edit(response: &str) -> (String, Option<String>) {
    let start_tag = "<DOCUMENT_EDIT>";
    let end_tag = "</DOCUMENT_EDIT>";

    if let Some(start) = response.find(start_tag) {
        if let Some(end) = response.find(end_tag) {
            let edit_content = response[start + start_tag.len()..end].trim().to_string();
            let before = response[..start].trim();
            let after = response[end + end_tag.len()..].trim();
            let message = format!("{}\n{}", before, after).trim().to_string();
            let display = if message.is_empty() {
                "✅ Document updated.".to_string()
            } else {
                format!("{}\n\n✅ Document updated.", message)
            };
            return (display, Some(edit_content));
        }
    }

    (response.to_string(), None)
}

/// Amend selected text based on user instructions.
#[tauri::command]
async fn ai_amend(
    selected_text: String,
    instruction: String,
    api_base: String,
    api_key: String,
    model: String,
) -> Result<String, String> {
    let llm = build_openai(&api_base, &api_key, &model);

    let system_prompt = format!(
        "You are a text editing assistant. The user has selected the following text:\n\
         ```\n{}\n```\n\n\
         Apply the user's requested changes and return ONLY the modified text. \
         Do not include any explanation, just the edited text. \
         Maintain the original formatting style (markdown if it was markdown).",
        selected_text
    );

    let prompt_template = message_formatter![
        fmt_message!(Message::new_system_message(&system_prompt)),
        fmt_template!(HumanMessagePromptTemplate::new(template_fstring!(
            "{input}", "input"
        )))
    ];

    let chain = LLMChainBuilder::new()
        .prompt(prompt_template)
        .llm(llm)
        .build()
        .map_err(|e| format!("Failed to build chain: {}", e))?;

    let result = chain
        .invoke(prompt_args! { "input" => instruction })
        .await
        .map_err(|e| format!("AI amend failed: {}", e))?;

    Ok(result)
}

#[derive(Clone, serde::Serialize)]
struct OpenFileResponse {
    path: String,
    content: String,
}

#[tauri::command]
async fn open_file(app: tauri::AppHandle) -> Result<Option<OpenFileResponse>, String> {
    let file_path = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .blocking_pick_file();
    match file_path {
        Some(fp) => {
            let path_buf = fp.into_path().map_err(|e| format!("Invalid path: {}", e))?;
            let path_str = path_buf.to_string_lossy().to_string();
            let content =
                fs::read_to_string(&path_buf).map_err(|e| format!("Failed to read: {}", e))?;

            Ok(Some(OpenFileResponse {
                path: path_str,
                content,
            }))
        }
        None => Ok(None),
    }
}

#[tauri::command]
async fn save_file(path: String, content: String) -> Result<(), String> {
    fs::write(&path, &content).map_err(|e| format!("Failed to save: {}", e))?;
    Ok(())
}

#[tauri::command]
async fn save_file_as(app: tauri::AppHandle, content: String) -> Result<Option<String>, String> {
    let file_path = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .blocking_save_file();
    match file_path {
        Some(fp) => {
            let path_buf = fp.into_path().map_err(|e| format!("Invalid path: {}", e))?;
            let path_str = path_buf.to_string_lossy().to_string();
            fs::write(&path_buf, &content).map_err(|e| format!("Failed to save: {}", e))?;

            Ok(Some(path_str))
        }
        None => Ok(None),
    }
}

// ── Window controls ──

#[tauri::command]
async fn window_minimize(window: tauri::WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|e| format!("{}", e))
}

#[tauri::command]
async fn window_toggle_maximize(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|e| format!("{}", e))
    } else {
        window.maximize().map_err(|e| format!("{}", e))
    }
}

#[tauri::command]
async fn window_close(window: tauri::WebviewWindow) -> Result<(), String> {
    window.close().map_err(|e| format!("{}", e))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .invoke_handler(tauri::generate_handler![
            open_file,
            save_file,
            save_file_as,
            window_minimize,
            window_toggle_maximize,
            window_close,
            load_ai_settings,
            save_ai_settings,
            verify_connection,
            list_models,
            ai_chat,
            ai_amend,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
