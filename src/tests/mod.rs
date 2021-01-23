mod rust {
    use std::path::PathBuf;

    use languageserver_types::{Position, TextDocumentIdentifier, TextDocumentPositionParams, Url};

    use crate::Result;
    use crate::{
        configs::language_configs,
        lsp::{LSClient, LSConfig},
    };

    fn get_client() -> Result<LSClient> {
        let config = language_configs()["rust"].clone();
        Ok(LSClient::spawn_server(
            "rust-analyzer".into(),
            None,
            PathBuf::from("/Users/zas/space/universal-lsif/src/tests/test_data/rust"),
        )
        .unwrap()
        .0)
    }

    fn client_with_document(path: &str) -> Result<LSClient> {
        let mut client = get_client()?;
        let src = std::fs::read_to_string(path)?;
        client.set_document(path, src);
        Ok(client)
    }

    #[test]
    fn test_server_init() {
        get_client().unwrap();
    }

    #[test]
    fn test_set_document() {
        client_with_document(
            "/Users/zas/space/universal-lsif/src/tests/test_data/rust/src/main.rs",
        )
        .unwrap();
    }

    #[test]
    fn test_get_definition() {
        let mut client = client_with_document(
            "/Users/zas/space/universal-lsif/src/tests/test_data/rust/src/main.rs",
        )
        .unwrap();

        std::thread::sleep(std::time::Duration::from_millis(3000));
        let def = client
            .get_definition(TextDocumentPositionParams {
                text_document: TextDocumentIdentifier {
                    uri: Url::from_file_path(
                        "/Users/zas/space/universal-lsif/src/tests/test_data/rust/src/main.rs",
                    )
                    .unwrap(),
                },
                position: Position {
                    line: 2,
                    character: 19,
                },
            })
            .unwrap();
        //dbg!(def);
    }
}

mod go {
    use std::path::PathBuf;

    use languageserver_types::{Position, TextDocumentIdentifier, TextDocumentPositionParams, Url};

    use crate::Result;
    use crate::{
        configs::language_configs,
        lsp::{LSClient, LSConfig},
    };

    fn get_client() -> Result<LSClient> {
        let config = language_configs()["go"].clone();
        Ok(LSClient::spawn_server(
            "gopls".into(),
            None,
            PathBuf::from("/Users/zas/space/universal-lsif/src/tests/test_data/go"),
        )
        .unwrap()
        .0)
    }

    #[test]
    fn test_server_init() {
        get_client().unwrap();
    }
}
