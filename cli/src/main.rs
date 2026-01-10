mod package;
mod validate;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "cargo-emporium")]
#[command(bin_name = "cargo-emporium")]
#[command(author, version, about = "CLI tool for packaging emporium extensions")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Subcommand for cargo integration (called as `cargo emporium`)
    Emporium {
        #[command(subcommand)]
        command: EmporiumCommands,
    },
}

#[derive(Subcommand)]
enum EmporiumCommands {
    /// Package an extension into an .empkg archive
    Package {
        /// Path to the extension directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,

        /// Output directory for the package (defaults to current directory)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Validate an extension manifest and WASM file
    Validate {
        /// Path to the extension directory (defaults to current directory)
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Emporium { command } => match command {
            EmporiumCommands::Package { path, output } => {
                let extension_path = path.unwrap_or_else(|| PathBuf::from("."));
                let output_path = output.unwrap_or_else(|| PathBuf::from("."));
                package::create_package(&extension_path, &output_path)?;
            }
            EmporiumCommands::Validate { path } => {
                let extension_path = path.unwrap_or_else(|| PathBuf::from("."));
                validate::validate_extension(&extension_path)?;
                println!("Extension is valid!");
            }
        },
    }

    Ok(())
}
