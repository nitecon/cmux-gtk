//! Standalone generator for shell completions and man page.
//! Usage: cargo run --bin cmux-generate
//! Outputs to packaging/completions/ and packaging/man/
//!
//! Only the command schema is shared; socket and update implementations are not
//! compiled into this generator.

#[path = "../cli/args.rs"]
mod args;

use clap::CommandFactory;
use clap_complete::{generate_to, Shell};
use clap_mangen::Man;
use std::fs;
use std::path::Path;

use args::Cli;

/// Generate CLI completions and normalized man-page output from the command schema.
fn main() -> std::io::Result<()> {
    let mut cmd = Cli::command();

    // Generate shell completions
    let comp_dir = Path::new("packaging/completions");
    fs::create_dir_all(comp_dir)?;

    for shell in [Shell::Bash, Shell::Zsh, Shell::Fish] {
        let path = generate_to(shell, &mut cmd, "cmux", comp_dir)?;
        eprintln!("Generated: {}", path.display());
    }

    // Generate man page
    let man_dir = Path::new("packaging/man");
    fs::create_dir_all(man_dir)?;

    let man = Man::new(cmd);
    let mut buf = Vec::new();
    man.render(&mut buf)?;
    let text = String::from_utf8(buf)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    let normalized = text
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(man_dir.join("cmux.1"), format!("{normalized}\n"))?;
    eprintln!("Generated: packaging/man/cmux.1");

    Ok(())
}
