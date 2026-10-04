#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    zephyr_lib::init_logging();
    zephyr_lib::run();
}
