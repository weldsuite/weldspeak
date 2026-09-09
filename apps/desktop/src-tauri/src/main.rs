// Suppress the console window on Windows release builds: a tray utility that
// opens a terminal on launch looks broken.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    weldspeak_desktop_lib::run()
}
