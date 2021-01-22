mod cli;
mod configs;
mod crawler;
mod emitter;
mod indexer;
mod lsif_data_cache;
mod lsp;
mod protocol;
mod tests;

use core::panic;
use std::{
    clone, env,
    path::{Path, PathBuf},
};

pub use anyhow::{anyhow as error, bail as ret_error, Error, Result};
use cli::Args;
use configs::language_configs;
use crawler::traverse;
use ignore::{DirEntry, Walk};
use indicatif::ProgressStyle;
use languageserver_types::{Position, TextDocumentIdentifier, TextDocumentPositionParams, Url};
use lsp::{LSClient, LSConfig};
use structopt::{clap::crate_authors, StructOpt};

fn main() {
    let args = env::args();
    // A hack to avoid sub-commands
    for arg in args {
        if &arg == "--langs" {
            println!("Supported languages:");
            for (language, config) in language_configs() {
                println!("    - {} using `{}`", language, config.start_command);
            }
            return;
        }
    }

    let mut args: Args = Args::from_args();
    args.canonicalize_paths();

    let config = match language_configs().get(&args.language) {
        Some(c) => c.clone(),
        None => {
            println!("\nLanguage not found, you can see the supported language using `universal-lsif --langs`");
            return;
        }
    };

    let (client, lsp_proc) = match LSClient::spawn_server(config.clone(), args.project_root.clone())
    {
        Ok(c) => c,
        Err(err) => {
            if let Some(installation_command) = &config.installation_command {
                println!("\n{}\nIf you haven't installed the language server, you can install it using `{}`", err, installation_command);
            } else {
                println!(
                    "\n{}\n Make sure you have installed the right language server: `{}`\n`",
                    err, config.start_command
                );
            }
            return;
        }
    };

    std::thread::sleep_ms(2000);
    crawler::traverse(args, client, config).unwrap();
    lsp_proc.join().unwrap();
}
