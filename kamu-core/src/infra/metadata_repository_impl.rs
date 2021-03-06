use super::*;
use crate::domain::*;
use crate::infra::serde::yaml::*;

use chrono::Utc;
use std::collections::HashSet;
use std::collections::LinkedList;
use std::convert::TryFrom;
use std::path::PathBuf;

pub struct MetadataRepositoryImpl {
    workspace_layout: WorkspaceLayout,
}

impl MetadataRepositoryImpl {
    pub fn new(workspace_layout: &WorkspaceLayout) -> Self {
        Self {
            workspace_layout: workspace_layout.clone(),
        }
    }

    fn get_all_datasets_impl(&self) -> Result<ListDatasetsIter, std::io::Error> {
        let read_dir = std::fs::read_dir(&self.workspace_layout.datasets_dir)?;
        Ok(ListDatasetsIter { rd: read_dir })
    }

    fn dataset_exists(&self, id: &DatasetID) -> bool {
        let path = self.get_dataset_metadata_dir(id);
        path.exists()
    }

    fn get_dataset_metadata_dir(&self, id: &DatasetID) -> PathBuf {
        self.workspace_layout.datasets_dir.join(id)
    }

    fn get_metadata_chain_impl(
        &self,
        dataset_id: &DatasetID,
    ) -> Result<MetadataChainImpl, DomainError> {
        let path = self.workspace_layout.datasets_dir.join(dataset_id.as_str());
        if !path.exists() {
            Err(DomainError::does_not_exist(
                ResourceKind::Dataset,
                (dataset_id as &str).to_owned(),
            ))
        } else {
            Ok(MetadataChainImpl::new(&path))
        }
    }

    fn sort_snapshots_in_dependency_order(
        &self,
        mut snapshots: LinkedList<DatasetSnapshot>,
    ) -> Vec<DatasetSnapshot> {
        let mut ordered = Vec::with_capacity(snapshots.len());
        let mut pending: HashSet<DatasetIDBuf> = snapshots.iter().map(|s| s.id.clone()).collect();
        let mut added: HashSet<DatasetIDBuf> = HashSet::new();

        // TODO: cycle detection
        while !snapshots.is_empty() {
            let head = snapshots.pop_front().unwrap();
            let has_deps = match head.source {
                DatasetSource::Derivative(ref src) => {
                    src.inputs.iter().any(|id| pending.contains(id))
                }
                _ => false,
            };
            if !has_deps {
                pending.remove(&head.id);
                added.insert(head.id.clone());
                ordered.push(head);
            } else {
                snapshots.push_back(head);
            }
        }
        ordered
    }
}

impl MetadataRepository for MetadataRepositoryImpl {
    fn get_all_datasets<'s>(&'s self) -> Box<dyn Iterator<Item = DatasetIDBuf> + 's> {
        Box::new(self.get_all_datasets_impl().unwrap())
    }

    fn add_dataset(&mut self, snapshot: DatasetSnapshot) -> Result<(), DomainError> {
        let dataset_metadata_dir = self.get_dataset_metadata_dir(&snapshot.id);

        if dataset_metadata_dir.exists() {
            return Err(DomainError::already_exists(
                ResourceKind::Dataset,
                String::from(&snapshot.id as &str),
            ));
        }

        let (kind, dependencies) = match snapshot.source {
            DatasetSource::Derivative(ref src) => {
                for input_id in src.inputs.iter() {
                    if !self.dataset_exists(input_id) {
                        return Err(DomainError::missing_reference(
                            ResourceKind::Dataset,
                            String::from(&snapshot.id as &str),
                            ResourceKind::Dataset,
                            String::from(input_id as &str),
                        ));
                    }
                }
                (DatasetKind::Derivative, src.inputs.clone())
            }
            DatasetSource::Root { .. } => (DatasetKind::Root, Vec::new()),
        };

        let first_block = MetadataBlock {
            block_hash: "".to_owned(),
            prev_block_hash: "".to_owned(),
            system_time: Utc::now(),
            source: Some(snapshot.source),
            output_slice: None,
            output_watermark: None,
            input_slices: None,
        };

        MetadataChainImpl::create(&dataset_metadata_dir, first_block).map_err(|e| e.into())?;

        let summary = DatasetSummary {
            id: snapshot.id.clone(),
            kind: kind,
            dependencies: dependencies,
            last_pulled: None,
            num_records: 0,
            data_size: 0,
            vocab: snapshot.vocab.unwrap_or_default(),
        };

        self.update_summary(&snapshot.id, summary)?;
        Ok(())
    }

    fn add_datasets(
        &mut self,
        snapshots: &mut dyn Iterator<Item = DatasetSnapshot>,
    ) -> Vec<(DatasetIDBuf, Result<(), DomainError>)> {
        let snapshots_ordered = self.sort_snapshots_in_dependency_order(snapshots.collect());

        snapshots_ordered
            .into_iter()
            .map(|s| {
                let id = s.id.clone();
                let res = self.add_dataset(s);
                (id, res)
            })
            .collect()
    }

    fn delete_dataset(&mut self, dataset_id: &DatasetID) -> Result<(), DomainError> {
        if !self.dataset_exists(dataset_id) {
            return Err(DomainError::does_not_exist(
                ResourceKind::Dataset,
                dataset_id.as_str().to_owned(),
            ));
        }

        // TODO: avoid copying
        let owned_id = dataset_id.to_owned();

        let dependents: Vec<_> = self
            .get_all_datasets_impl()
            .unwrap()
            .filter(|id| id != dataset_id)
            .map(|id| self.get_summary(&id).unwrap())
            .filter(|s| s.dependencies.contains(&owned_id))
            .map(|s| s.id)
            .collect();

        if dependents.len() > 0 {
            return Err(DomainError::dangling_reference(
                dependents
                    .into_iter()
                    .map(|id| (ResourceKind::Dataset, id.as_str().to_owned()))
                    .collect(),
                ResourceKind::Dataset,
                dataset_id.as_str().to_owned(),
            ));
        }

        // TODO: should be handled differently
        let metadata_dir = self.get_dataset_metadata_dir(dataset_id);
        let volume_layout = VolumeLayout::new(&self.workspace_layout.local_volume_dir);
        let layout = DatasetLayout::new(&volume_layout, dataset_id);

        let paths = [
            layout.cache_dir,
            layout.checkpoints_dir,
            layout.data_dir,
            metadata_dir,
        ];

        for p in paths.iter() {
            if p.exists() {
                std::fs::remove_dir_all(p).unwrap_or_else(|e| {
                    panic!("Failed to remove directory {}: {}", p.display(), e)
                });
            }
        }

        Ok(())
    }

    fn get_metadata_chain(
        &self,
        dataset_id: &DatasetID,
    ) -> Result<Box<dyn MetadataChain>, DomainError> {
        self.get_metadata_chain_impl(dataset_id)
            .map(|c| Box::new(c) as Box<dyn MetadataChain>)
    }

    fn get_summary(&self, dataset_id: &DatasetID) -> Result<DatasetSummary, DomainError> {
        let path = self
            .workspace_layout
            .datasets_dir
            .join(dataset_id)
            .join("summary");
        if !path.exists() {
            Err(DomainError::does_not_exist(
                ResourceKind::Dataset,
                dataset_id.as_str().to_owned(),
            ))
        } else {
            let file = std::fs::File::open(&path).unwrap_or_else(|e| {
                panic!(
                    "Failed to open the summary file at {}: {}",
                    path.display(),
                    e
                )
            });

            let manifest: Manifest<DatasetSummary> =
                serde_yaml::from_reader(&file).unwrap_or_else(|e| {
                    panic!(
                        "Failed to deserialize the DatasetSummary at {}: {}",
                        path.display(),
                        e
                    )
                });

            assert_eq!(manifest.kind, "DatasetSummary");
            Ok(manifest.content)
        }
    }

    // TODO: summaries should be per branch
    // TODO: vocab should be stored in the chain
    // TODO: update summary lazily when new blocks appear
    fn update_summary(
        &mut self,
        dataset_id: &DatasetID,
        summary: DatasetSummary,
    ) -> Result<(), DomainError> {
        let path = self
            .workspace_layout
            .datasets_dir
            .join(dataset_id)
            .join("summary");

        let file = std::fs::File::create(&path).map_err(|e| InfraError::from(e).into())?;

        let manifest = Manifest {
            api_version: 1,
            kind: "DatasetSummary".to_owned(),
            content: summary,
        };

        serde_yaml::to_writer(file, &manifest).map_err(|e| InfraError::from(e).into())?;
        Ok(())
    }
}

///////////////////////////////////////////////////////////////////////////////
// Used by get_all_datasets
///////////////////////////////////////////////////////////////////////////////

struct ListDatasetsIter {
    rd: std::fs::ReadDir,
}

impl Iterator for ListDatasetsIter {
    type Item = DatasetIDBuf;
    fn next(&mut self) -> Option<Self::Item> {
        let res = self.rd.next()?;
        let path = res.unwrap();
        let name = path.file_name();
        Some(DatasetIDBuf::try_from(&name).unwrap())
    }
}
