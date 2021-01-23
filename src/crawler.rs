use std::{
    fs::File,
    hash::Hasher,
    path::{Path, PathBuf},
    sync::{mpsc::channel, Arc, Mutex},
};

use anyhow::Context;
use ignore::{DirEntry, Walk};
use indicatif::ProgressBar;
use languageserver_types::{
    request::GotoDefinitionResponse, Position, Range as LspRange, TextDocumentIdentifier,
    TextDocumentPositionParams, Url,
};
use lazy_static::lazy_static;
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use regex::Regex;

use crate::{
    cli::Args,
    emitter::file_emitter::FileEmitter,
    indexer::Indexer,
    lsp::{LSClient, LSConfig},
    protocol, Result,
};

pub fn traverse(args: Args, mut client: LSClient, config: LSConfig) -> Result<()> {
    let (def_tx, def_rx) = channel();
    let (ref_tx, ref_rx) = channel();

    let (file_emitter, flush_signal) = FileEmitter::new(get_output_file(&args)?);

    let a = args.clone();
    let c = config.clone();
    let indexer_proc = std::thread::spawn(move || -> Result<()> {
        Indexer::index(a, c, file_emitter, def_rx, ref_rx)
    });

    let pb = ProgressBar::new(
        paths(&args.project_root.clone().unwrap(), config.extensions.clone()).len() as u64,
    );
    pb.set_message("Waiting for the language server to finish indexing");

    for p in paths(&args.project_root.clone().unwrap(), config.extensions.clone()) {
        let text = std::fs::read_to_string(&p).unwrap();

        client.set_document(&p, text.clone());

        get_words(text)
            .into_iter()
            .try_for_each(|(word, range)| -> Result<()> {
                if config.keywords.get(&word).is_some() {
                    return Ok(());
                }

                let (start, _end) = (range.start, range.end);

                match client.get_definition(TextDocumentPositionParams {
                    text_document: TextDocumentIdentifier {
                        uri: Url::from_file_path(&p).unwrap(),
                    },
                    position: start,
                }) {
                    Ok(resp) => {
                        let def_location = match resp {
                            GotoDefinitionResponse::Scalar(it) => Some(it),
                            GotoDefinitionResponse::Array(it) => it.get(0).map(Clone::clone),
                            GotoDefinitionResponse::Link(_) => None,
                        };
                        if def_location.is_none() {
                            return Ok(());
                        }
                        let def_location = def_location.unwrap();

                        if def_location.range.start == start
                            && Url::from_file_path(&p).unwrap().to_string()
                                == def_location.uri.to_string()
                        {
                            // it defines itself, so it's a declaration
                            def_tx.send(Definition {
                                location: Location {
                                    file_path: def_location.uri.to_string(),
                                    range: Range { lsp_range: range },
                                },
                                node_name: word.clone(),
                                comment: None,
                            })?;
                        } else {
                            ref_tx.send(Reference {
                                location: Location {
                                    file_path: Url::from_file_path(&p).unwrap().to_string(),
                                    range: Range { lsp_range: range },
                                },
                                node_name: word.clone(),
                                def: Definition {
                                    location: Location {
                                        file_path: def_location.uri.to_string(),
                                        range: Range {
                                            lsp_range: def_location.range,
                                        },
                                    },
                                    node_name: word,
                                    comment: None,
                                },
                            })?;
                        }
                    }
                    Err(_err) => {
                        //dbg!(err);
                    }
                }

                Ok(())
            })?;

        pb.inc(1);
    }

    drop(def_tx);
    drop(ref_tx);
    indexer_proc.join().unwrap()?;
    flush_signal.recv()?;

    Ok(())
}

fn get_words(text: String) -> Vec<(String, LspRange)> {
    let mut res = Vec::new();
    for (idx, line) in text.split('\n').enumerate() {
        lazy_static! {
            static ref RE: Regex = Regex::new("\\w+(?:'\\w+)*").unwrap();
        }

        for m in RE.find_iter(line) {
            let range = LspRange {
                start: Position {
                    line: idx as u64,
                    character: m.start() as u64,
                },
                end: Position {
                    line: idx as u64,
                    character: m.end() as u64,
                },
            };
            res.push((m.as_str().to_string(), range));
        }
    }
    res
}

fn get_output_file(args: &Args) -> Result<File> {
    let output = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .open(&args.output.clone().unwrap())
        .context("Could not open the output file")?;
    output
        .set_len(0)
        .context("Could not clear the output file")?;
    Ok(output)
}

#[derive(Debug, Clone)]
pub struct Definition {
    pub location: Location,
    pub node_name: String,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Reference {
    pub location: Location,
    pub node_name: String,
    pub def: Definition,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Location {
    pub file_path: String,
    pub range: Range,
}

impl Location {
    /// Returns the name of the file (the final component of the file path)
    pub fn file_name(&self) -> String {
        PathBuf::from(&self.file_path)
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct Range {
    lsp_range: protocol::types::Range,
}

impl std::hash::Hash for Range {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.lsp_range.start.line.hash(state);
        self.lsp_range.start.character.hash(state);
    }
}

impl Definition {
    pub fn range(&self) -> protocol::types::Range {
        self.location.range.range()
    }
}

impl Reference {
    pub fn range(&self) -> protocol::types::Range {
        self.location.range.range()
    }
}

impl Range {
    pub fn range(&self) -> protocol::types::Range {
        self.lsp_range.clone()
    }
}

pub fn paths<P: AsRef<Path>>(root: P, extensions: Vec<String>) -> Vec<PathBuf> {
    Walk::new(root)
        .into_iter()
        .filter_map(Result::ok)
        .filter(move |entry| {
            entry.metadata().unwrap().is_file() && matches_extensions(entry, &extensions)
        })
        .map(DirEntry::into_path)
        .collect()
}

/// Returns true if the given `DirEntry` has an extension equal to one of
/// the given extensions, and false otherwise.
fn matches_extensions(dir_entry: &DirEntry, extensions: &[String]) -> bool {
    extensions.iter().any(|ex| has_extension(dir_entry, ex))
}

/// Returns true if the given `DirEntry`'s extension is equal to the given
/// extension.
fn has_extension(dir_entry: &DirEntry, target_ext: &str) -> bool {
    dir_entry
        .path()
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e == target_ext)
        .unwrap_or(false)
}

mod tests {
    use crate::Result;

    use super::get_words;

    #[test]
    fn test_for_each_word() {
        let text = r#"
            let value = a.b.c();
            for_each_word(text.to_string(), |word, _range| -> Result<()> {
                words.push(word.to_string());
                Ok(())
            }
        "#;
        let mut words = Vec::new();
        get_words(text.to_string())
            .into_iter()
            .try_for_each(|(word, _range)| -> Result<()> {
                words.push(word.to_string());
                Ok(())
            })
            .unwrap();

        assert_eq!(
            words,
            vec![
                "let",
                "value",
                "a",
                "b",
                "c",
                "for_each_word",
                "text",
                "to_string",
                "word",
                "_range",
                "Result",
                "words",
                "push",
                "word",
                "to_string",
                "Ok"
            ]
            .into_iter()
            .map(|it| it.to_string())
            .collect::<Vec<_>>()
        );
    }

    #[test]
    fn test_for_each_word_2() {
        let text = r#"
        type a struct {
            b   c.d
            e f
            g        h
        }
        "#;
        let mut words = Vec::new();
        get_words(text.to_string())
            .into_iter()
            .try_for_each(|(word, _range)| -> Result<()> {
                words.push(word.to_string());
                Ok(())
            })
            .unwrap();

        assert_eq!(
            words,
            vec!["type", "a", "struct", "b", "c", "d", "e", "f", "g", "h"]
                .into_iter()
                .map(|it| it.to_string())
                .collect::<Vec<_>>()
        );
    }
}
