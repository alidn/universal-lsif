use std::{
    collections::HashSet,
    io::{BufReader, BufWriter, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{channel, Receiver},
    thread::JoinHandle,
};

use anyhow::Context;
use jsonrpc_lite::{Id, JsonRpc, Params};
use languageserver_types::{
    notification::{DidOpenTextDocument, Initialized, Notification},
    request::GotoDefinitionResponse,
    ClientCapabilities, DidOpenTextDocumentParams, Hover, InitializeParams, InitializeResult,
    InitializedParams, TextDocumentItem, TextDocumentPositionParams, TraceOption, Url,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_derive::*;
use serde_json::{json, Value};

use crate::{protocol::types::HoverResult, Result};

use self::parse_helpers::read_message;

mod parse_helpers;

/// A language-server client.
pub struct LSClient {
    pub message_rx: Receiver<String>,
    writer: Box<dyn Write + Send>,
    next_id: u64,
}

impl LSClient {
    pub fn spawn_server(
        start_command: String,
        start_args: Option<String>,
        root_path: PathBuf,
    ) -> Result<(Self, JoinHandle<()>)> {
        let args = start_args
            .map(|it| {
                let split = it.split(' ').map(|it| it.to_string()).collect::<Vec<_>>();
                split
            })
            .unwrap_or(Vec::new());
        let mut process = Command::new(start_command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .context(format!("Failed to spawn the language server with command"))?;

        let mut stdout = process.stdout;

        let (message_tx, message_rx) = channel();

        let lsp_proc = std::thread::Builder::new()
            .name("lsp-stdout-looper".into())
            .spawn(move || {
                let mut reader = Box::new(BufReader::new(stdout.take().unwrap()));
                loop {
                    match read_message(&mut reader) {
                        Ok(message_str) => {
                            if message_tx.send(message_str).is_err() {
                                // Receiver was dropped, end the loop
                                break;
                            };
                        }
                        Err(err) => panic!("Failed to read message: {}", err),
                    };
                }
            })?;

        let writer = Box::new(BufWriter::new(process.stdin.take().unwrap()));

        let mut ls_client = Self {
            writer,
            message_rx,
            next_id: 0,
        };

        let init_params = InitializeParams {
            process_id: Some(u64::from(std::process::id())),
            initialization_options: None,
            capabilities: ClientCapabilities::default(),
            trace: Some(TraceOption::Verbose),
            workspace_folders: None,
            root_uri: Some(Url::from_directory_path(root_path).unwrap()),
            root_path: None,
        };

        let rpc_params = Params::from(serde_json::to_value(init_params)?);
        let request = JsonRpc::request_with_params(
            Id::Num(ls_client.next_id as i64),
            "initialize",
            rpc_params,
        );

        ls_client.next_id += 1;

        ls_client.send_rpc(&serde_json::to_value(&request)?);

        ls_client.await_response::<InitializeResult>()?;
        ls_client.send_lsp_notification::<Initialized>(InitializedParams {});

        Ok((ls_client, lsp_proc))
    }

    pub fn set_document<P: AsRef<Path>>(&mut self, path: P, text: String) {
        let params = DidOpenTextDocumentParams {
            text_document: TextDocumentItem {
                uri: Url::from_file_path(path).unwrap(),
                language_id: String::new(),
                version: 0,
                text,
            },
        };
        self.send_lsp_notification::<DidOpenTextDocument>(params);
    }

    fn send_lsp_notification<N>(&mut self, params: N::Params)
    where
        N: Notification,
        N::Params: Serialize,
    {
        let json_params = Params::from(serde_json::to_value(params).unwrap());
        self.send_notification(N::METHOD, json_params);
    }

    fn send_notification(&mut self, method: &str, params: Params) {
        let notification = JsonRpc::notification_with_params(method, params);
        let res = serde_json::to_value(&notification).unwrap();
        self.send_rpc(&res);
    }

    pub fn get_definition(
        &mut self,
        lsp_params: TextDocumentPositionParams,
    ) -> Result<GotoDefinitionResponse> {
        let rpc_params = Params::from(serde_json::to_value(lsp_params)?);
        let request = JsonRpc::request_with_params(
            Id::Num(self.next_id as i64),
            "textDocument/definition",
            rpc_params,
        );

        self.next_id += 1;

        self.send_rpc(&serde_json::to_value(&request)?);

        let resp = self.await_response::<GotoDefinitionResponse>()?;
        Ok(resp)
    }

    fn await_response<T: DeserializeOwned>(&mut self) -> Result<T> {
        let result;
        loop {
            let message = self.message_rx.recv()?;
            if let Some((_id, res)) = self.handle_message(&message) {
                result = Some(res.with_context(|| {
                    format!("Language server failed with message: `{}`", message)
                })?);
                break;
            } else {
                //dbg!(message);
            }
        }
        let result = result.unwrap();
        let resp: T = serde_json::from_value(result)?;
        Ok(resp)
    }

    fn handle_message(
        &mut self,
        message: &str,
    ) -> Option<(u64, std::result::Result<Value, jsonrpc_lite::Error>)> {
        match JsonRpc::parse(message) {
            Ok(JsonRpc::Request(obj)) => {
                //dbg!(obj);
                return None;
            }
            Ok(value @ JsonRpc::Notification(_)) => {
                //dbg!(value);
                return None;
            }
            Ok(value @ JsonRpc::Success(_)) => {
                let id = number_from_id(&value.get_id().unwrap());
                let result = value.get_result().unwrap();
                return Some((id, Ok(result.clone())));
            }
            Ok(value @ JsonRpc::Error(_)) => {
                let id = number_from_id(&value.get_id().unwrap());
                let error = value.get_error().unwrap();
                return Some((id, Err(error.clone())));
            }
            Err(err) => panic!("Error in parsing incoming string: {}", err),
        }
        None
    }

    fn send_rpc(&mut self, value: &Value) {
        let rpc = match prepare_lsp_json(value) {
            Ok(r) => r,
            Err(err) => panic!("Encoding Error {:?}", err),
        };

        self.write(rpc.as_ref());
    }

    fn write(&mut self, message: &str) {
        self.writer.write_all(message.as_bytes()).expect(
            "error writing to stdin for language server,
        ",
        );
        self.writer
            .flush()
            .expect("error flushing child stdin for language server")
    }
}

/// Prepare Language Server Protocol style JSON String from
/// a serde_json object `Value`
fn prepare_lsp_json(msg: &Value) -> Result<String, serde_json::error::Error> {
    let request = serde_json::to_string(&msg)?;
    Ok(format!(
        "Content-Length: {}\r\n\r\n{}",
        request.len(),
        request
    ))
}

/// Configuration info for running a language server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LSConfig {
    pub extensions: Vec<String>,
    pub keywords: HashSet<String>,
}

fn number_from_id(id: &Id) -> u64 {
    match *id {
        Id::Num(n) => n as u64,
        Id::Str(ref s) => u64::from_str_radix(s, 10).expect("failed to convert string id to u64"),
        _ => panic!("unexpected value for id: None"),
    }
}
