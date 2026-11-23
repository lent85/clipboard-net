#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    clipboard_net_lib::run();
}
