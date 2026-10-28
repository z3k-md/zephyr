#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if zephyr_lib::run_helper_if_asked() {
        return;
    }
    zephyr_lib::init_logging();
    zephyr_lib::run();
}
