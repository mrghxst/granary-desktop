// Release builds are GUI-subsystem apps (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    granary_lib::run();
}
