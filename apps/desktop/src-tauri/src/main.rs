// Suppress the console window on Windows release builds so launching from the
// Start menu opens the Hub, not a terminal.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    weldspeak_desktop_lib::run()
}
