#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::fs;
use tauri_plugin_dialog::DialogExt;

/// Response payload returned by `open_file` containing the file path and its contents.
#[derive(Clone, serde::Serialize)]
struct OpenFileResponse {
    path: String,
    content: String,
}

/// Opens a file picker dialog filtered to Markdown files, reads the selected file,
/// and returns its path and content.
///
/// # Returns
/// - `Ok(Some(OpenFileResponse))` if a file was selected and read successfully
/// - `Ok(None)` if the user cancelled the dialog
/// - `Err(String)` if file reading failed
#[tauri::command]
async fn open_file(app: tauri::AppHandle) -> Result<Option<OpenFileResponse>, String> {
    let file_path = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .blocking_pick_file();

    match file_path {
        Some(fp) => {
            let path_buf = fp
                .into_path()
                .map_err(|e| format!("Invalid file path: {}", e))?;
            let path_str = path_buf.to_string_lossy().to_string();
            let content = fs::read_to_string(&path_buf)
                .map_err(|e| format!("Failed to read file: {}", e))?;
            Ok(Some(OpenFileResponse {
                path: path_str,
                content,
            }))
        }
        None => Ok(None),
    }
}

/// Saves content to an existing file path.
///
/// # Arguments
/// - `path` — The absolute file path to write to
/// - `content` — The text content to write
#[tauri::command]
async fn save_file(path: String, content: String) -> Result<(), String> {
    fs::write(&path, &content).map_err(|e| format!("Failed to save file: {}", e))
}

/// Opens a "Save As" dialog filtered to Markdown files, writes the content to the
/// chosen path, and returns the new file path.
///
/// # Arguments
/// - `content` — The text content to save
///
/// # Returns
/// - `Ok(Some(String))` with the chosen file path on success
/// - `Ok(None)` if the user cancelled the dialog
#[tauri::command]
async fn save_file_as(app: tauri::AppHandle, content: String) -> Result<Option<String>, String> {
    let file_path = app
        .dialog()
        .file()
        .add_filter("Markdown", &["md", "markdown", "txt"])
        .blocking_save_file();

    match file_path {
        Some(fp) => {
            let path_buf = fp
                .into_path()
                .map_err(|e| format!("Invalid file path: {}", e))?;
            let path_str = path_buf.to_string_lossy().to_string();
            fs::write(&path_buf, &content)
                .map_err(|e| format!("Failed to save file: {}", e))?;
            Ok(Some(path_str))
        }
        None => Ok(None),
    }
}

/// Minimizes the current application window.
#[tauri::command]
async fn window_minimize(window: tauri::WebviewWindow) -> Result<(), String> {
    window.minimize().map_err(|e| format!("Failed to minimize: {}", e))
}

/// Toggles maximize state of the current application window.
#[tauri::command]
async fn window_toggle_maximize(window: tauri::WebviewWindow) -> Result<(), String> {
    if window.is_maximized().unwrap_or(false) {
        window.unmaximize().map_err(|e| format!("Failed to unmaximize: {}", e))
    } else {
        window.maximize().map_err(|e| format!("Failed to maximize: {}", e))
    }
}

/// Closes the current application window.
#[tauri::command]
async fn window_close(window: tauri::WebviewWindow) -> Result<(), String> {
    window.close().map_err(|e| format!("Failed to close: {}", e))
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
