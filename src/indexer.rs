use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{
        mpsc::{channel, Receiver},
        Arc,
    },
};

use anyhow::Result;
use ignore::{DirEntry, Walk};
use languageserver_types::{NumberOrString, Url};
use serde_json::to_string;

use crate::{
    cli::Args,
    crawler::{paths, Definition, Reference},
    edge,
    emitter::emitter::Emitter,
    lsif_data_cache::{DefinitionInfo, LsifDataCache},
    lsp::LSConfig,
    protocol::types::{
        Contents, DefinitionResult, Document, Edge, EdgeData, HoverResult, LSIFMarkedString,
        Language, MetaData, Moniker, ReferenceResult, ResultSet, ToolInfo, ID,
    },
};

pub struct Indexer<E>
where
    E: Emitter,
{
    emitter: E,
    tool_info: ToolInfo,
    opt: Args,
    config: LSConfig,

    project_id: ID,

    cache: LsifDataCache,

    cached_file_paths: Option<Vec<PathBuf>>,
}

impl<E> Indexer<E>
where
    E: Emitter,
{
    /// Generates an LSIF dump from a project by traversing through files of the given language
    /// and emitting the LSIF equivalent using the given emitter.
    pub fn index(
        opt: Args,
        config: LSConfig,
        emitter: E,
        def_rx: Receiver<Definition>,
        ref_rx: Receiver<Reference>,
    ) -> Result<()> {
        let mut indexer = Self {
            emitter,
            config,
            tool_info: ToolInfo::default(),
            opt: opt.clone(),
            project_id: 0,
            cache: LsifDataCache::default(),
            cached_file_paths: Default::default(),
        };

        indexer.emit_metadata_and_project_vertex();
        indexer.emit_documents();
        indexer.emit_defs_and_refs(def_rx, ref_rx);
        indexer.link_reference_results_to_ranges();
        indexer.emit_contains();

        indexer.emitter.end();

        Ok(())
    }

    /// Emits the contains relationship for all documents and the ranges that they contain.
    fn emit_contains(&mut self) {
        let documents = self.cache.get_documents();
        for d in documents {
            let all_range_ids = [&d.reference_range_ids[..], &d.definition_range_ids[..]].concat();
            if !all_range_ids.is_empty() {
                self.emitter.emit_edge(Edge::contains(d.id, all_range_ids));
            }
        }
        self.emit_contains_for_project();
    }

    /// Emits a contains edge between a document and its ranges.
    fn emit_contains_for_project(&mut self) {
        let document_ids = self.cache.get_documents().map(|d| d.id).collect();
        self.emitter
            .emit_edge(Edge::contains(self.project_id, document_ids));
    }

    /// Emits item relations for each indexed definition result value.
    fn link_reference_results_to_ranges(&mut self) {
        let def_infos = self.cache.get_mut_def_infos();
        Self::link_items_to_definitions(&def_infos.collect(), &mut self.emitter);
    }

    /// Adds item relations between the given definition range and the ranges that
    /// define and reference it.
    fn link_items_to_definitions(def_infos: &Vec<&mut DefinitionInfo>, emitter: &mut E) {
        for d in def_infos {
            let ref_result_id = emitter.emit_vertex(ReferenceResult {});

            emitter.emit_edge(edge!(References, d.result_set_id -> ref_result_id));
            emitter.emit_edge(Edge::def_item(
                ref_result_id,
                vec![d.range_id],
                d.document_id,
            ));

            for (document_id, range_ids) in &d.reference_range_ids {
                emitter.emit_edge(Edge::ref_item(
                    ref_result_id,
                    range_ids.clone(),
                    *document_id,
                ));
            }
        }
    }

    fn emit_defs_and_refs(&mut self, def_rx: Receiver<Definition>, ref_rx: Receiver<Reference>) {
        for def in def_rx {
            //dbg!(&def);
            self.index_definition(def);
        }

        for r in ref_rx {
            //dbg!(&r);
            self.index_reference(r);
        }
    }

    /// Emits data for the given reference object and caches it for emitting 'contains' later.
    fn index_reference(&mut self, r: Reference) {
        self.index_reference_to_definition(&r.def, &r);
    }

    /// Returns a range identifier for the given reference. If a range for the object has
    /// not been emitted, a new vertex is created.
    fn ensure_range_for(&mut self, r: &Reference) -> ID {
        match self
            .cache
            .get_range_id(&r.location.file_path, &r.location.range)
        {
            Some(range_id) => range_id,
            None => {
                let range_id = self.emitter.emit_vertex(r.range());
                self.cache.cache_reference_range(r, range_id);
                range_id
            }
        }
    }

    /// Emits data for the given reference object that is defined within
    /// an index target package.
    fn index_reference_to_definition(&mut self, def: &Definition, r: &Reference) {
        // 1. Emit/Get vertices(s)
        let range_id = self.ensure_range_for(r);

        // 2. Connect the emitted vertices
        let next_edge = {
            let def_result_set_id = match self.cache.get_definition_info(&def.location) {
                Some(it) => it.result_set_id,
                None => return,
            };
            edge!(Next, range_id -> def_result_set_id)
        };
        self.emitter.emit_edge(next_edge);

        // 3. Cache the result
        self.cache.cache_reference(&def, &r, range_id);
    }

    /// Emits data for the given definition object and caches it for
    /// emitting 'contains' later.
    fn index_definition(&mut self, def: Definition) {
        let document_id = match self.cache.get_document_id(&def.location.file_path) {
            Some(it) => it,
            None => return,
        };

        // 1. Emit Vertices
        let range_id = self.emitter.emit_vertex(def.range());
        let result_set_id = self.emitter.emit_vertex(ResultSet {});
        let def_result_id = self.emitter.emit_vertex(DefinitionResult {});
        let hover_result_id = def.comment.clone().map(|c| {
            self.emitter.emit_vertex(HoverResult {
                result: Contents {
                    contents: vec![LSIFMarkedString {
                        language: self.opt.language.to_string(),
                        value: c,
                        is_raw_string: true,
                    }],
                },
            })
        });
        let moniker_id = self.emitter.emit_vertex(Moniker {
            kind: "local".to_string(),
            scheme: "zas".to_string(),
            identifier: format!("{}:{}", def.location.file_name(), def.node_name.clone()),
        });

        // 2. Connect the emitted vertices
        let next_edge = edge!(Next, range_id -> result_set_id);
        let definition_edge = edge!(Definition, result_set_id -> def_result_id);
        let item_edge = Edge::item(def_result_id, vec![range_id], document_id);
        let moniker_edge = edge!(Moniker, result_set_id -> moniker_id);

        for edge in vec![next_edge, definition_edge, item_edge, moniker_edge].into_iter() {
            self.emitter.emit_edge(edge);
        }

        if let Some(id) = hover_result_id {
            self.emitter.emit_edge(edge!(Hover, result_set_id -> id));
        }

        // 3. Cache the result
        self.cache
            .cache_definition(&def, document_id, range_id, result_set_id);
    }

    /// Emits a metadata and project vertex. This method caches the identifier of the project
    /// vertex, which is needed to construct the project/document contains relation later.
    fn emit_metadata_and_project_vertex(&mut self) {
        self.project_id = self.emitter.emit_vertex(MetaData {
            version: "0.1".into(),
            position_encoding: "utf-16".into(),
            tool_info: Some(self.tool_info.clone()),
            project_root: Url::from_directory_path(&self.opt.project_root.clone().unwrap()).unwrap(),
        });
    }

    fn emit_documents(&mut self) {
        self.file_paths().iter().for_each(|filepath| {
            let document_id = self.emitter.emit_vertex(Document {
                uri: Url::from_file_path(&filepath).unwrap(),
                language_id: self.opt.language.clone(),
            });
            self.cache.cache_document(
                Url::from_file_path(filepath).unwrap().to_string(),
                document_id,
            );
        });
    }

    /// Returns a `Vec` of of paths of all the files that have the same format as this
    /// indexer's language.
    fn file_paths(&mut self) -> Vec<PathBuf> {
        if let Some(res) = &self.cached_file_paths {
            return res.clone();
        }

        let exs = self.config.extensions.clone();
        let res = paths(&self.opt.project_root.clone().unwrap(), exs);
        self.cached_file_paths = Some(res.clone());
        res
    }
}
