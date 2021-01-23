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
    let mut args: Args = Args::from_args();
    args.canonicalize_paths();

    let config = match language_configs().get(&args.language) {
        Some(c) => c.clone(),
        None => {
            eprintln!("Failed: Language not found.");
            return;
        }
    };

    let (client, lsp_proc) = match LSClient::spawn_server(
        args.init_server_command.clone(),
        args.server_args.clone(),
        args.project_root.clone().unwrap(),
    ) {
        Ok(c) => c,
        Err(err) => {
            eprintln!("Failed: {}", err);
            return;
        }
    };

    // A hack to make sure the server is initialized
    std::thread::sleep(std::time::Duration::from_millis(1500));

    crawler::traverse(args, client, config).unwrap();
    lsp_proc.join().unwrap();
}
