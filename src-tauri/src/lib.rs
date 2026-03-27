#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use langchain_rust::{
    agent::{AgentExecutor, OpenAiToolAgentBuilder},
    chain::{options::ChainCallOptions, Chain, LLMChainBuilder},
    fmt_message, fmt_template,
    llm::openai::{OpenAI, OpenAIConfig},
    memory::SimpleMemory,
    message_formatter,
    prompt::HumanMessagePromptTemplate,
    prompt_args,
    schemas::Message,
    template_fstring,
    schemas::memory::BaseMemory,
    tools::Tool,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
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
#[serde(rename_all = "camelCase")]
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

// ── Document editing tools for the AI agent ──

/// Shared state between the `ai_chat` command and its tools.
/// Created fresh for each invocation so tools can read/modify the document.
#[derive(Clone)]
struct DocumentState {
    /// The current document content. Tools read and write through this.
    content: Arc<std::sync::Mutex<String>>,
    /// Set to `true` if any tool modified the document during this agent run.
    was_edited: Arc<std::sync::Mutex<bool>>,
}

/// Replace the entire document with new content.
struct EditDocumentTool {
    state: DocumentState,
}

#[async_trait]
impl Tool for EditDocumentTool {
    fn name(&self) -> String {
        "edit_document".to_string()
    }

    fn description(&self) -> String {
        "Replace the entire document content with new markdown content. \
         Use this for large structural changes, full rewrites, or when \
         creating a new document from scratch."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The complete new document content in markdown"
                }
            },
            "required": ["content"]
        })
    }

    async fn parse_input(&self, input: &str) -> Value {
        serde_json::from_str(input).unwrap_or_else(|_| json!({"content": input}))
    }

    async fn run(&self, input: Value) -> Result<String, Box<dyn std::error::Error>> {
        let new_content = input["content"]
            .as_str()
            .ok_or("Missing 'content' parameter")?;
        *self.state.content.lock().unwrap() = new_content.to_string();
        *self.state.was_edited.lock().unwrap() = true;
        Ok("Document replaced successfully.".to_string())
    }
}

/// Find specific text in the document and replace it.
struct FindAndReplaceTool {
    state: DocumentState,
}

#[async_trait]
impl Tool for FindAndReplaceTool {
    fn name(&self) -> String {
        "find_and_replace".to_string()
    }

    fn description(&self) -> String {
        "Find specific text in the document and replace it. Use this for \
         targeted edits like fixing typos, changing specific phrases, or \
         updating particular sections without rewriting the whole document."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "find": {
                    "type": "string",
                    "description": "The exact text to find in the document"
                },
                "replace": {
                    "type": "string",
                    "description": "The replacement text"
                }
            },
            "required": ["find", "replace"]
        })
    }

    async fn parse_input(&self, input: &str) -> Value {
        serde_json::from_str(input).unwrap_or_else(|_| json!({"input": input}))
    }

    async fn run(&self, input: Value) -> Result<String, Box<dyn std::error::Error>> {
        let find = input["find"]
            .as_str()
            .ok_or("Missing 'find' parameter")?;
        let replace = input["replace"]
            .as_str()
            .ok_or("Missing 'replace' parameter")?;

        let mut content = self.state.content.lock().unwrap();
        let count = content.matches(find).count();
        if count == 0 {
            return Ok(format!(
                "Text not found in document. The exact text '{}' does not appear. \
                 Check the text and try again.",
                find
            ));
        }
        *content = content.replace(find, replace);
        *self.state.was_edited.lock().unwrap() = true;
        Ok(format!(
            "Replaced {} occurrence(s) of the specified text.",
            count
        ))
    }
}

/// Insert text at a specific line number.
struct InsertAtTool {
    state: DocumentState,
}

#[async_trait]
impl Tool for InsertAtTool {
    fn name(&self) -> String {
        "insert_at".to_string()
    }

    fn description(&self) -> String {
        "Insert text at a specific line number in the document. \
         Line numbers start at 1. The new text is inserted before \
         the specified line."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "line": {
                    "type": "integer",
                    "description": "The line number to insert before (1-indexed)"
                },
                "text": {
                    "type": "string",
                    "description": "The text to insert (may include newlines for multiple lines)"
                }
            },
            "required": ["line", "text"]
        })
    }

    async fn parse_input(&self, input: &str) -> Value {
        serde_json::from_str(input).unwrap_or_else(|_| json!({"input": input}))
    }

    async fn run(&self, input: Value) -> Result<String, Box<dyn std::error::Error>> {
        let line = input["line"]
            .as_u64()
            .ok_or("Missing or invalid 'line' parameter")? as usize;
        let text = input["text"]
            .as_str()
            .ok_or("Missing 'text' parameter")?;

        let mut content = self.state.content.lock().unwrap();
        let mut lines: Vec<&str> = content.lines().collect();

        // Clamp to valid range (1-indexed, insert before the line)
        let idx = if line == 0 {
            0
        } else {
            (line - 1).min(lines.len())
        };

        // Insert each line of the new text
        let new_lines: Vec<&str> = text.lines().collect();
        for (i, new_line) in new_lines.iter().enumerate() {
            lines.insert(idx + i, new_line);
        }

        *content = lines.join("\n");
        *self.state.was_edited.lock().unwrap() = true;
        Ok(format!(
            "Inserted {} line(s) at line {}.",
            new_lines.len(),
            line
        ))
    }
}

/// Delete a range of lines from the document.
struct DeleteLinesTool {
    state: DocumentState,
}

#[async_trait]
impl Tool for DeleteLinesTool {
    fn name(&self) -> String {
        "delete_lines".to_string()
    }

    fn description(&self) -> String {
        "Delete a range of lines from the document. Line numbers are \
         1-indexed. Both start and end are inclusive."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "start_line": {
                    "type": "integer",
                    "description": "First line to delete (1-indexed, inclusive)"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Last line to delete (1-indexed, inclusive)"
                }
            },
            "required": ["start_line", "end_line"]
        })
    }

    async fn parse_input(&self, input: &str) -> Value {
        serde_json::from_str(input).unwrap_or_else(|_| json!({"input": input}))
    }

    async fn run(&self, input: Value) -> Result<String, Box<dyn std::error::Error>> {
        let start = input["start_line"]
            .as_u64()
            .ok_or("Missing 'start_line' parameter")? as usize;
        let end = input["end_line"]
            .as_u64()
            .ok_or("Missing 'end_line' parameter")? as usize;

        if start == 0 || end == 0 || start > end {
            return Ok("Invalid line range. start_line and end_line must be >= 1, and start_line <= end_line.".to_string());
        }

        let mut content = self.state.content.lock().unwrap();
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let s = (start - 1).min(total);
        let e = end.min(total);
        let deleted = e - s;

        let remaining: Vec<&str> = lines[..s]
            .iter()
            .chain(lines[e..].iter())
            .copied()
            .collect();
        *content = remaining.join("\n");
        *self.state.was_edited.lock().unwrap() = true;
        Ok(format!("Deleted {} line(s) (lines {}-{}).", deleted, start, end))
    }
}

/// Replace a range of lines with new content.
struct ReplaceLinesTool {
    state: DocumentState,
}

#[async_trait]
impl Tool for ReplaceLinesTool {
    fn name(&self) -> String {
        "replace_lines".to_string()
    }

    fn description(&self) -> String {
        "Replace a range of lines in the document with new content. \
         Line numbers are 1-indexed and both start and end are inclusive. \
         Use this for precise block-level editing when you know the exact \
         line numbers."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "start_line": {
                    "type": "integer",
                    "description": "First line to replace (1-indexed, inclusive)"
                },
                "end_line": {
                    "type": "integer",
                    "description": "Last line to replace (1-indexed, inclusive)"
                },
                "new_content": {
                    "type": "string",
                    "description": "The replacement text (may include newlines for multiple lines)"
                }
            },
            "required": ["start_line", "end_line", "new_content"]
        })
    }

    async fn parse_input(&self, input: &str) -> Value {
        serde_json::from_str(input).unwrap_or_else(|_| json!({"input": input}))
    }

    async fn run(&self, input: Value) -> Result<String, Box<dyn std::error::Error>> {
        let start = input["start_line"]
            .as_u64()
            .ok_or("Missing 'start_line' parameter")? as usize;
        let end = input["end_line"]
            .as_u64()
            .ok_or("Missing 'end_line' parameter")? as usize;
        let new_text = input["new_content"]
            .as_str()
            .ok_or("Missing 'new_content' parameter")?;

        if start == 0 || end == 0 || start > end {
            return Ok("Invalid line range. start_line and end_line must be >= 1, and start_line <= end_line.".to_string());
        }

        let mut content = self.state.content.lock().unwrap();
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let s = (start - 1).min(total);
        let e = end.min(total);

        let mut result: Vec<&str> = lines[..s].to_vec();
        let new_lines: Vec<&str> = new_text.lines().collect();
        result.extend_from_slice(&new_lines);
        result.extend_from_slice(&lines[e..]);
        *content = result.join("\n");
        *self.state.was_edited.lock().unwrap() = true;
        Ok(format!(
            "Replaced lines {}-{} with {} new line(s).",
            start,
            end,
            new_lines.len()
        ))
    }
}

/// Read the current document content with line numbers.
struct GetDocumentTool {
    state: DocumentState,
}

#[async_trait]
impl Tool for GetDocumentTool {
    fn name(&self) -> String {
        "get_document".to_string()
    }

    fn description(&self) -> String {
        "Read the current document content with line numbers. \
         Always call this first before making line-based edits \
         so you have accurate line numbers. Useful to see the \
         latest state after making edits in a multi-step session."
            .to_string()
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn parse_input(&self, input: &str) -> Value {
        serde_json::from_str(input).unwrap_or_else(|_| json!({}))
    }

    async fn run(&self, _input: Value) -> Result<String, Box<dyn std::error::Error>> {
        let content = self.state.content.lock().unwrap().clone();
        if content.is_empty() {
            Ok("(empty document)".to_string())
        } else {
            // Return content with line numbers for precise editing
            let numbered: String = content
                .lines()
                .enumerate()
                .map(|(i, line)| format!("{}| {}", i + 1, line))
                .collect::<Vec<_>>()
                .join("\n");
            Ok(numbered)
        }
    }
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
/// When `edit_mode` is true, the AI uses tool calling to edit the document.
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

    let doc_display = if document_content.is_empty() {
        "(empty document)"
    } else {
        &document_content
    };
    let history_display = if history.is_empty() {
        "(new conversation)"
    } else {
        &history
    };

    if edit_mode {
        // ── Agent path: AI can edit the document via tool calls ──

        let doc_state = DocumentState {
            content: Arc::new(std::sync::Mutex::new(document_content.clone())),
            was_edited: Arc::new(std::sync::Mutex::new(false)),
        };

        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(GetDocumentTool {
                state: doc_state.clone(),
            }),
            Arc::new(FindAndReplaceTool {
                state: doc_state.clone(),
            }),
            Arc::new(InsertAtTool {
                state: doc_state.clone(),
            }),
            Arc::new(ReplaceLinesTool {
                state: doc_state.clone(),
            }),
            Arc::new(DeleteLinesTool {
                state: doc_state.clone(),
            }),
            Arc::new(EditDocumentTool {
                state: doc_state.clone(),
            }),
        ];

        let system_prompt = format!(
            "You are an expert markdown writing assistant embedded in WriterMD, a markdown editor.\n\
             You help the user write, edit, and structure their documents.\n\n\
             CURRENT DOCUMENT:\n```markdown\n{}\n```\n\n\
             CONVERSATION HISTORY:\n{}\n\n\
             EDITING WORKFLOW:\n\
             1. ALWAYS call get_document first to see the latest content with line numbers.\n\
             2. Choose the right tool for the job:\n\
                - find_and_replace: best for fixing typos, changing specific phrases, renaming things.\n\
                - replace_lines: best for rewriting a specific section when you know the line numbers.\n\
                - insert_at: best for adding new content at a specific location.\n\
                - delete_lines: best for removing sections.\n\
                - edit_document: ONLY for full rewrites or creating a document from scratch.\n\
             3. You can chain multiple tool calls for complex, multi-step edits.\n\
             4. After editing, always confirm what changed in a brief, natural response.\n\n\
             IMPORTANT RULES:\n\
             - Prefer surgical tools (find_and_replace, replace_lines) over edit_document.\n\
             - For find_and_replace, use exact text matches from the document.\n\
             - Be concise in your responses. Don't repeat the entire document back.",
            doc_display, history_display
        );

        // Pre-populate memory with conversation history
        let mut memory = SimpleMemory::new();
        for msg in &recent[..recent.len().saturating_sub(1)] {
            match msg.role.as_str() {
                "user" => memory.add_user_message(&msg.content),
                "assistant" => memory.add_ai_message(&msg.content),
                _ => {}
            }
        }

        let agent = OpenAiToolAgentBuilder::new()
            .tools(&tools)
            .prefix(&system_prompt)
            .options(ChainCallOptions::new().with_max_tokens(4096))
            .build(llm)
            .map_err(|e| format!("Failed to build agent: {}", e))?;

        let memory_arc: Arc<tokio::sync::Mutex<dyn langchain_rust::schemas::memory::BaseMemory>> =
            Arc::new(tokio::sync::Mutex::new(memory));

        let executor = AgentExecutor::from_agent(agent)
            .with_memory(memory_arc)
            .with_max_iterations(8);

        let result = executor
            .invoke(prompt_args! { "input" => user_message })
            .await
            .map_err(|e| format!("AI chat failed: {}", e))?;

        // Read back shared state to see if the document was edited
        let was_edited = *doc_state.was_edited.lock().unwrap();
        let (document_edit, diff) = if was_edited {
            let new_content = doc_state.content.lock().unwrap().clone();
            let diff = similar::TextDiff::from_lines(&document_content, &new_content)
                .unified_diff()
                .context_radius(3)
                .to_string();
            (Some(new_content), Some(diff))
        } else {
            (None, None)
        };

        let message = if was_edited && !result.is_empty() {
            format!("{}\n\n✅ Document updated.", result)
        } else if was_edited {
            "✅ Document updated.".to_string()
        } else {
            result
        };

        Ok(ChatResponse {
            message,
            document_edit,
            diff,
        })
    } else {
        // ── Simple chain path: chat-only, no document editing ──

        let system_prompt = format!(
            "You are an expert markdown writing assistant embedded in WriterMD, a markdown editor.\n\
             You help the user write, edit, and structure their documents.\n\n\
             CURRENT DOCUMENT:\n```markdown\n{}\n```\n\n\
             CONVERSATION HISTORY:\n{}\n\n\
             INSTRUCTIONS:\n\
             - Respond conversationally to help the user with their document.\n\
             - IMPORTANT: You are in CHAT-ONLY mode. Do NOT edit the document. \
             Only discuss, advise, and answer questions about the document.",
            doc_display, history_display
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

        Ok(ChatResponse {
            message: result,
            document_edit: None,
            diff: None,
        })
    }
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
