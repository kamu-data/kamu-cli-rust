use std::backtrace::Backtrace;
use thiserror::Error;

#[derive(Debug)]
pub enum ResourceKind {
    Dataset,
}

#[derive(Error, Debug)]
pub enum DomainError {
    #[error("{kind:?} {id} does not exist")]
    DoesNotExist {
        kind: ResourceKind,
        id: String,
        backtrace: Backtrace,
    },
    #[error("{kind:?} {id} already exists")]
    AlreadyExists {
        kind: ResourceKind,
        id: String,
        backtrace: Backtrace,
    },
    #[error("{from_kind:?} {from_id} references non existent {to_kind:?} {to_id}")]
    MissingReference {
        from_kind: ResourceKind,
        from_id: String,
        to_kind: ResourceKind,
        to_id: String,
        backtrace: Backtrace,
    },
    #[error("{to_kind:?} {to_id} is referenced by {from_kinds_ids:?}")]
    DanglingReference {
        from_kinds_ids: Vec<(ResourceKind, String)>,
        to_kind: ResourceKind,
        to_id: String,
        backtrace: Backtrace,
    },
    #[error("{0}")]
    InfraError(Box<dyn std::error::Error>),
}

impl DomainError {
    pub fn already_exists(kind: ResourceKind, id: String) -> DomainError {
        DomainError::AlreadyExists {
            kind: kind,
            id: id,
            backtrace: Backtrace::capture(),
        }
    }

    pub fn does_not_exist(kind: ResourceKind, id: String) -> DomainError {
        DomainError::DoesNotExist {
            kind: kind,
            id: id,
            backtrace: Backtrace::capture(),
        }
    }

    pub fn missing_reference(
        from_kind: ResourceKind,
        from_id: String,
        to_kind: ResourceKind,
        to_id: String,
    ) -> DomainError {
        DomainError::MissingReference {
            from_kind: from_kind,
            from_id: from_id,
            to_kind: to_kind,
            to_id: to_id,
            backtrace: Backtrace::capture(),
        }
    }
}