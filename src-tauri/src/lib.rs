pub mod analyzer;
mod commands;
pub mod config;
pub mod dump;
pub mod export;
pub mod mcp;
pub mod models;
pub mod parser;
mod watcher;

use mcp::McpState;
use watcher::WatcherState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .manage(WatcherState::new())
        .manage(McpState::new())
        .setup(|app| {
            // Enable logging in both debug and release builds.
            // Debug: Info level (verbose, for development).
            // Release: Warn level (captures errors and warnings from the file watcher).
            let level = if cfg!(debug_assertions) {
                log::LevelFilter::Info
            } else {
                log::LevelFilter::Warn
            };
            app.handle().plugin(
                tauri_plugin_log::Builder::default()
                    .level(level)
                    .build(),
            )?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::detect_noteplan_path,
            commands::scan,
            commands::system_dump,
            commands::export_assessment_context,
            commands::get_note_content,
            commands::open_noteplan_url,
            commands::get_git_rev,
            commands::get_daily_notes,
            commands::get_content_blocks,
            commands::get_filing_targets,
            commands::get_filing_suggestions,
            watcher::start_watching,
            watcher::stop_watching,
            watcher::is_watching,
            mcp::commands::mcp_connect,
            mcp::commands::mcp_disconnect,
            mcp::commands::mcp_status,
            mcp::commands::mcp_call_tool,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
