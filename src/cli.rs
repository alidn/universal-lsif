use std::path::{Component, Path, PathBuf};

use structopt::StructOpt;

use crate::protocol::types::Language;

/// Represents the command-line arguments.
#[derive(Clone, Debug, StructOpt)]
#[structopt(
    name = "universal-lsif",
    about = "An LSIF indexer for every language (use --langs to see supported language)"
)]
pub struct Args {
    /// Command for starting the language server.
    /// This client and the server communicate over stdin.
    pub init_server_command: String,
    /// Specifies the language
    pub language: String,
    /// Optional arguments for running the language server.
    #[structopt(short, long)]
    pub server_args: Option<String>,
    /// Path to the root of the project, or the current directory if not present.
    #[structopt(parse(from_os_str))]
    pub project_root: Option<PathBuf>,
    /// The output file, `dump.json` if not present.
    #[structopt(short, long, parse(from_os_str))]
    pub output: Option<PathBuf>,
}

impl Args {
    pub fn canonicalize_paths(&mut self) {
        self.project_root = Some(
            self.project_root
                .clone()
                .unwrap_or(PathBuf::from("."))
                .canonicalize()
                .unwrap(),
        );
        self.output = Some(
            self.output.as_ref().map_or(
                normalize_path(
                    &self
                        .project_root
                        .clone()
                        .unwrap()
                        .join(PathBuf::from("dump.json")),
                ),
                |p| normalize_path(p),
            ),
        );
    }
}

/// Same as `std::path::Path::canonicalize`, but does not require that the given path exists.
pub fn normalize_path(path: &Path) -> PathBuf {
    let mut components = path.components().peekable();
    let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().cloned() {
        components.next();
        PathBuf::from(c.as_os_str())
    } else {
        PathBuf::new()
    };

    for component in components {
        match component {
            Component::Prefix(..) => unreachable!(),
            Component::RootDir => {
                ret.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                ret.pop();
            }
            Component::Normal(c) => {
                ret.push(c);
            }
        }
    }
    ret
}
