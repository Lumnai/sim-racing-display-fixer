// Hide the console window in release; --fix / --install-task / --remove-task run headless.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |flag: &str| args.iter().any(|a| a == flag);

    if has("--fix") {
        lunis_display_fixer_lib::run_fix_headless();
        return;
    }
    if has("--install-task") {
        lunis_display_fixer_lib::install_task();
        return;
    }
    if has("--remove-task") {
        lunis_display_fixer_lib::remove_task();
        return;
    }
    lunis_display_fixer_lib::run();
}
