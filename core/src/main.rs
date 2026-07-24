//! displaycore - thin CLI over the engine, for testing the native heal headlessly.
//!   status | capture | fix | restart

use lunis_display_core::{adapter, ccd, engine, profile};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(String::as_str).unwrap_or("status");
    let path = profile::default_path();

    match cmd {
        "status" => {
            let s = engine::status(&path);
            println!("current  : {}px", s.current_width);
            println!("target   : {:?}", s.target_width);
            println!("has_prof : {}", s.has_profile);
            println!("matches  : {}", s.matches);
            println!("adapters : {}", s.adapters.join(", "));
            println!("summary  : {}", s.summary);
        }
        "capture" => match ccd::query() {
            Ok(snap) => match profile::save(&snap, &path) {
                Ok(()) => println!(
                    "saved profile ({}px, {} paths) -> {}",
                    snap.max_source_width(),
                    snap.paths.len(),
                    path.display()
                ),
                Err(e) => eprintln!("save error: {e}"),
            },
            Err(e) => eprintln!("query error: {e}"),
        },
        "fix" => {
            let r = engine::fix(&path, |line| println!("{line}"));
            println!("[{:?}] {}", r.outcome, r.message);
            if r.outcome == engine::FixOutcome::Failed {
                std::process::exit(1);
            }
        }
        "restart" => {
            let has_nvidia = adapter::list()
                .unwrap_or_default()
                .iter()
                .any(|n| n.to_uppercase().contains("NVIDIA"));
            let filter = if has_nvidia { Some("NVIDIA") } else { None };
            println!(
                "before: {}px",
                ccd::query().map(|s| s.max_source_width()).unwrap_or(0)
            );
            match adapter::restart(filter, |l| println!("{l}")) {
                Ok(n) => println!("toggled {n} adapter(s)"),
                Err(e) => eprintln!("restart error: {e}"),
            }
            println!(
                "after : {}px",
                ccd::query().map(|s| s.max_source_width()).unwrap_or(0)
            );
        }
        _ => println!("usage: displaycore status|capture|fix|restart"),
    }
}
