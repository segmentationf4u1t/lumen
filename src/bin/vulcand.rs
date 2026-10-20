#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(e) = vulcan::daemon::run() {
        eprintln!("vulcand error: {e}");
        std::process::exit(1);
    }
}
