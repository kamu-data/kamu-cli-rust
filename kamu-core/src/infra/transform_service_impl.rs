use crate::domain::*;
use crate::infra::serde::yaml::*;
use crate::infra::*;

use slog::{info, Logger};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

pub struct TransformServiceImpl {
    metadata_repo: Rc<RefCell<dyn MetadataRepository>>,
    engine_factory: Arc<Mutex<EngineFactory>>,
    volume_layout: VolumeLayout,
    logger: Logger,
}

impl TransformServiceImpl {
    pub fn new(
        metadata_repo: Rc<RefCell<dyn MetadataRepository>>,
        engine_factory: Arc<Mutex<EngineFactory>>,
        volume_layout: &VolumeLayout,
        logger: Logger,
    ) -> Self {
        Self {
            metadata_repo: metadata_repo,
            engine_factory: engine_factory,
            volume_layout: volume_layout.clone(),
            logger: logger,
        }
    }

    // Note: Can be called from multiple threads
    fn do_transform(
        request: ExecuteQueryRequest,
        meta_chain: Box<dyn MetadataChain>,
        listener: Arc<Mutex<dyn TransformListener>>,
        engine_factory: Arc<Mutex<EngineFactory>>,
    ) -> Result<TransformResult, TransformError> {
        listener.lock().unwrap().begin();

        match Self::do_transform_inner(request, meta_chain, engine_factory) {
            Ok(res) => {
                listener.lock().unwrap().success(&res);
                Ok(res)
            }
            Err(err) => {
                listener.lock().unwrap().error(&err);
                Err(err)
            }
        }
    }

    // Note: Can be called from multiple threads
    fn do_transform_inner(
        request: ExecuteQueryRequest,
        mut meta_chain: Box<dyn MetadataChain>,
        engine_factory: Arc<Mutex<EngineFactory>>,
    ) -> Result<TransformResult, TransformError> {
        let prev_hash = meta_chain.read_ref(&BlockRef::Head).unwrap();

        let engine = engine_factory
            .lock()
            .unwrap()
            .get_engine(&request.source.transform.engine)?;

        let result = engine.lock().unwrap().transform(request)?;

        let new_block = MetadataBlock {
            prev_block_hash: prev_hash,
            ..result.block
        };
        let block_hash = meta_chain.append(new_block);

        Ok(TransformResult::Updated {
            block_hash: block_hash,
        })
    }

    pub fn get_next_operation(
        &self,
        dataset_id: &DatasetID,
    ) -> Result<Option<ExecuteQueryRequest>, DomainError> {
        let output_chain = self.metadata_repo.borrow().get_metadata_chain(dataset_id)?;

        // TODO: limit traversal depth
        let mut sources: Vec<_> = output_chain
            .iter_blocks()
            .filter_map(|b| b.source)
            .collect();

        // TODO: source could've changed several times
        if sources.len() > 1 {
            unimplemented!("Transform evolution is not yet supported");
        }

        let source = match sources.pop().unwrap() {
            DatasetSource::Derivative(src) => src,
            _ => panic!("Transform called on non-derivative dataset {}", dataset_id),
        };

        let mut non_empty = 0;
        let input_slices: BTreeMap<_, _> = source
            .inputs
            .iter()
            .enumerate()
            .map(|(index, input_id)| {
                let (slice, empty) = self
                    .get_input_slice(index, input_id, output_chain.as_ref())
                    .unwrap();

                if !empty {
                    non_empty += 1;
                }

                (input_id.clone(), slice)
            })
            .collect();

        let mut vocabs: BTreeMap<_, _> = source
            .inputs
            .iter()
            .map(|input_id| {
                (
                    input_id.clone(),
                    self.metadata_repo
                        .borrow()
                        .get_summary(input_id)
                        .unwrap()
                        .vocab,
                )
            })
            .collect();

        vocabs.insert(
            dataset_id.to_owned(),
            self.metadata_repo
                .borrow()
                .get_summary(dataset_id)
                .unwrap()
                .vocab,
        );

        let output_layout = DatasetLayout::new(&self.volume_layout, dataset_id);

        let mut data_dirs: BTreeMap<_, _> = source
            .inputs
            .iter()
            .map(|input_id| {
                (
                    input_id.clone(),
                    DatasetLayout::new(&self.volume_layout, &input_id).data_dir,
                )
            })
            .collect();

        data_dirs.insert(dataset_id.to_owned(), output_layout.data_dir);

        if non_empty > 0 {
            Ok(Some(ExecuteQueryRequest {
                dataset_id: dataset_id.to_owned(),
                checkpoints_dir: output_layout.checkpoints_dir, // TODO: move down a layer
                source: source,
                dataset_vocabs: vocabs,
                input_slices: input_slices,
                data_dirs: data_dirs, // TODO: move down a layer
            }))
        } else {
            Ok(None)
        }
    }

    // TODO: Avoid iterating through output chain multiple times
    fn get_input_slice(
        &self,
        index: usize,
        dataset_id: &DatasetID,
        output_chain: &dyn MetadataChain,
    ) -> Result<(InputDataSlice, bool), DomainError> {
        // Determine processed data range
        // Result is either: () or (inf, upper] or (lower, upper]
        let iv_processed = output_chain
            .iter_blocks()
            .filter_map(|b| b.input_slices)
            .map(|mut ss| ss.remove(index).interval)
            .find(|iv| !iv.is_empty())
            .unwrap_or(TimeInterval::empty());

        // Determine unprocessed data range
        // Result is either: (-inf, inf) or (lower, inf)
        let iv_unprocessed = iv_processed.right_complement();

        let input_chain = self.metadata_repo.borrow().get_metadata_chain(dataset_id)?;

        // Filter unprocessed input blocks
        let blocks_unprocessed: Vec<_> = input_chain
            .iter_blocks()
            .take_while(|b| iv_unprocessed.contains_point(&b.system_time))
            .collect();

        // Determine available data/watermark range
        // Result is either: () or (-inf, upper]
        let iv_available = blocks_unprocessed
            .first()
            .map(|b| TimeInterval::unbounded_closed_right(b.system_time.clone()))
            .unwrap_or(TimeInterval::empty());

        // Result is either: () or (lower, upper]
        let iv_to_process = iv_available.intersect(&iv_unprocessed);

        let explicit_watermarks: Vec<_> = blocks_unprocessed
            .iter()
            .rev()
            .filter(|b| b.output_watermark.is_some())
            .map(|b| Watermark {
                system_time: b.system_time.clone(),
                event_time: b.output_watermark.unwrap().clone(),
            })
            .collect();

        let empty = !blocks_unprocessed.iter().any(|b| b.output_slice.is_some())
            && explicit_watermarks.is_empty();

        Ok((
            InputDataSlice {
                interval: iv_to_process,
                explicit_watermarks: explicit_watermarks,
            },
            empty,
        ))
    }

    fn update_summary(
        &self,
        dataset_id: &DatasetID,
        result: &TransformResult,
    ) -> Result<(), TransformError> {
        match result {
            TransformResult::UpToDate => Ok(()),
            TransformResult::Updated { block_hash } => {
                let mut metadata_repo = self.metadata_repo.borrow_mut();

                let mut summary = metadata_repo
                    .get_summary(dataset_id)
                    .map_err(|e| TransformError::internal(e))?;

                let block = metadata_repo
                    .get_metadata_chain(dataset_id)
                    .unwrap()
                    .get_block(block_hash)
                    .unwrap();

                summary.num_records = match block.output_slice {
                    Some(slice) => summary.num_records + slice.num_records as u64,
                    _ => 0,
                };

                summary.last_pulled = Some(block.system_time);

                let layout = DatasetLayout::new(&self.volume_layout, dataset_id);
                summary.data_size = fs_extra::dir::get_size(layout.data_dir).unwrap_or(0);
                summary.data_size += fs_extra::dir::get_size(layout.checkpoints_dir).unwrap_or(0);

                metadata_repo
                    .update_summary(dataset_id, summary)
                    .map_err(|e| TransformError::internal(e))
            }
        }
    }
}

impl TransformService for TransformServiceImpl {
    fn transform(
        &mut self,
        dataset_id: &DatasetID,
        maybe_listener: Option<Arc<Mutex<dyn TransformListener>>>,
    ) -> Result<TransformResult, TransformError> {
        let null_listener = Arc::new(Mutex::new(NullTransformListener {}));
        let listener = maybe_listener.unwrap_or(null_listener);

        info!(self.logger, "Transforming single dataset"; "dataset" => dataset_id.as_str());

        // TODO: There might be more operations to do
        if let Some(request) = self
            .get_next_operation(dataset_id)
            .map_err(|e| TransformError::internal(e))?
        {
            let meta_chain = self
                .metadata_repo
                .borrow()
                .get_metadata_chain(&dataset_id)
                .unwrap();

            let res =
                Self::do_transform(request, meta_chain, listener, self.engine_factory.clone())?;
            self.update_summary(dataset_id, &res)?;
            Ok(res)
        } else {
            Ok(TransformResult::UpToDate)
        }
    }

    fn transform_multi(
        &mut self,
        dataset_ids: &mut dyn Iterator<Item = &DatasetID>,
        maybe_multi_listener: Option<Arc<Mutex<dyn TransformMultiListener>>>,
    ) -> Vec<(DatasetIDBuf, Result<TransformResult, TransformError>)> {
        let null_multi_listener = Arc::new(Mutex::new(NullTransformMultiListener {}));
        let multi_listener = maybe_multi_listener.unwrap_or(null_multi_listener);

        let dataset_ids_owned: Vec<_> = dataset_ids.map(|id| id.to_owned()).collect();
        info!(self.logger, "Transforming multiple datasets"; "datasets" => ?dataset_ids_owned);

        // TODO: handle errors without crashing
        let requests: Vec<_> = dataset_ids_owned
            .into_iter()
            .map(|dataset_id| {
                let next_op = self
                    .get_next_operation(&dataset_id)
                    .map_err(|e| TransformError::internal(e))
                    .unwrap();
                (dataset_id, next_op)
            })
            .collect();

        let mut results: Vec<(DatasetIDBuf, Result<TransformResult, TransformError>)> =
            Vec::with_capacity(requests.len());

        let thread_handles: Vec<_> = requests
            .into_iter()
            .filter_map(|(dataset_id, maybe_request)| match maybe_request {
                None => {
                    results.push((dataset_id, Ok(TransformResult::UpToDate)));
                    None
                }
                Some(request) => {
                    let null_listener = Arc::new(Mutex::new(NullTransformListener {}));
                    let listener = multi_listener
                        .lock()
                        .unwrap()
                        .begin_transform(&dataset_id)
                        .unwrap_or(null_listener);
                    let meta_chain = self
                        .metadata_repo
                        .borrow()
                        .get_metadata_chain(&dataset_id)
                        .unwrap();
                    let engine_factory = self.engine_factory.clone();

                    let thread_handle = std::thread::Builder::new()
                        .name("transform_multi".to_owned())
                        .spawn(move || {
                            let res =
                                Self::do_transform(request, meta_chain, listener, engine_factory);
                            (dataset_id, res)
                        })
                        .unwrap();

                    Some(thread_handle)
                }
            })
            .collect();

        results.extend(thread_handles.into_iter().map(|h| h.join().unwrap()));

        results
            .iter()
            .filter(|(_, res)| res.is_ok())
            .for_each(|(dataset_id, res)| {
                self.update_summary(dataset_id, res.as_ref().unwrap())
                    .unwrap()
            });

        results
    }
}
