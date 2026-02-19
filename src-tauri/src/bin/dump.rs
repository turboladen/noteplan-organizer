//! CLI tool to dump a comprehensive overview of the NotePlan JD system structure.
//!
//! Run with: cargo run --bin dump --manifest-path src-tauri/Cargo.toml
//!
//! Note: On macOS, this requires Full Disk Access for the terminal app
//! since NotePlan's data directory is sandboxed. If the output shows 0 notes,
//! use the in-app `system_dump` command instead (which runs inside the Tauri
//! process with the necessary permissions).

use app_lib::config;
use app_lib::dump;
use app_lib::parser::scan_noteplan_dir;

fn main() {
    let path = config::detect_noteplan_path().unwrap_or_else(|| {
        eprintln!("ERROR: Could not find NotePlan data directory.");
        eprintln!("Checked:");
        eprintln!("  - App Store container");
        eprintln!("  - Setapp container");
        eprintln!("  - iCloud Drive");
        std::process::exit(1);
    });

    let store = scan_noteplan_dir(&path);
    let report = dump::generate_dump(&store, &path);
    print!("{}", report);
}
