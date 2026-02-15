mod analyzer;
mod commands;
mod config;
mod models;
mod parser;
mod watcher;

use watcher::WatcherState;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(WatcherState::new())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::detect_noteplan_path,
            commands::scan,
            commands::get_note_content,
            commands::open_noteplan_url,
            watcher::start_watching,
            watcher::stop_watching,
            watcher::is_watching,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
