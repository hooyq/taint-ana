//! `cargo taint-ana $FLAGS $ARGS` calls `cargo build` with RUSTC_WRAPPER set to `taint-ana`.
//! The flags are passed to `taint-ana` through env var `TAINT_ANA_FLAGS`.
//! The remaining args are unchanged.
//! To re-run `cargo taint-ana` with different flags on the same crate, please `cargo clean` first.
use std::env;
use std::ffi::OsString;
use std::process::Command;

const CARGO_TAINT_ANA_HELP: &str = r#"Extract function signatures from Rust project
Usage:
    cargo taint-ana [options] [--] [<cargo build options>...]
Common options:
    -h, --help               Print this message
    -V, --version            Print version info and exit
    
Options after the first "--" are the same arguments that `cargo build` accepts.

Examples:
    # Extract function signatures from the project
    cargo taint-ana
    # With specific target
    cargo +nightly taint-ana -- --target x86_64-unknown-linux-gnu
"#;

fn show_help() {
    println!("{}", CARGO_TAINT_ANA_HELP);
}

fn show_version() {
    println!("taint-ana 0.1.0");
}

fn cargo() -> Command {
    Command::new(env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo")))
}

// Determines whether a `--flag` is present.
fn has_arg_flag(name: &str) -> bool {
    let mut args = std::env::args().take_while(|val| val != "--");
    args.any(|val| val == name)
}

fn in_cargo_taint_ana() {
    // Now we run `cargo build $FLAGS $ARGS`, giving the user the
    // chance to add additional arguments. `FLAGS` is set to identify
    // this target. The user gets to control what gets actually passed to taint-ana.
    let mut cmd = cargo();
    cmd.arg("build");
    cmd.env("RUSTC_WRAPPER", "taint-ana");
    cmd.env("RUST_BACKTRACE", "full");

    // Pass TAINT_ANA_LOG if specified by the user. Default to info if not specified.
    const TAINT_ANA_LOG: &str = "TAINT_ANA_LOG";
    let log_level = env::var(TAINT_ANA_LOG).ok();
    cmd.env(TAINT_ANA_LOG, log_level.as_deref().unwrap_or("info"));

    let mut args = std::env::args().skip(2);

    let flags: Vec<_> = args.by_ref().take_while(|arg| arg != "--").collect();
    let flags = flags.join(" ");
    cmd.env("TAINT_ANA_FLAGS", flags);

    let exit_status = cmd
        .args(args)
        .spawn()
        .expect("could not run cargo")
        .wait()
        .expect("failed to wait for cargo?");
    if !exit_status.success() {
        std::process::exit(exit_status.code().unwrap_or(-1))
    };
}

fn main() {
    if has_arg_flag("--help") || has_arg_flag("-h") {
        show_help();
        return;
    }
    if has_arg_flag("--version") || has_arg_flag("-V") {
        show_version();
        return;
    }
    if let Some("taint-ana") = std::env::args().nth(1).as_deref() {
        in_cargo_taint_ana();
    }
}

