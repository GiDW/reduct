#![windows_subsystem = "windows"]

mod batch;
mod paths;
mod progress;
mod summary;
mod ui;

use std::ffi::OsString;
use std::path::Path;

fn main() {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    std::process::exit(dispatch(&args));
}

/// No arguments opens the settings dialog; `--batch <listfile>` runs a batch
/// with the progress window, and `--batch <listfile> --no-window` without it
/// (the CI smoke test). Anything else exits with `2`.
fn dispatch(args: &[OsString]) -> i32 {
    let (list, window) = match args {
        [] => return ui::run_settings(),
        [flag, list] if flag == "--batch" => (list, true),
        [flag, list, no_window] if flag == "--batch" && no_window == "--no-window" => (list, false),
        _ => return 2,
    };
    batch::install_quiet_panic_hook();
    batch::run(Path::new(list), window)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<OsString> {
        list.iter().map(OsString::from).collect()
    }

    #[test]
    fn unknown_arguments_exit_with_2() {
        assert_eq!(dispatch(&args(&["--frobnicate"])), 2);
        assert_eq!(dispatch(&args(&["--batch"])), 2);
        assert_eq!(dispatch(&args(&["--batch", "a", "b"])), 2);
        assert_eq!(dispatch(&args(&["--no-window", "--batch", "a"])), 2);
        assert_eq!(dispatch(&args(&["--batch", "a", "--no-window", "b"])), 2);
    }
}
