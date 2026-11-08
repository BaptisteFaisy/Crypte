// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let early = match crypte_lib::cli::parse_early(&argv) {
        Ok(early) => early,
        Err(usage) => {
            eprint!("{usage}");
            std::process::exit(2);
        }
    };
    if let Some(code) = crypte_lib::cli::run_early(&early) {
        std::process::exit(code);
    }
    crypte_lib::run(crypte_lib::cli::events_path(&early));
}
