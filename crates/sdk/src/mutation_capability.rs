use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMutationSurface {
    WriteWithOperation,
    WriteWithoutOperation,
    LongTermMutateWithOperation,
    LongTermMutateWithoutOperation,
    FinalizeTurn,
    MemoryGovernancePolicy,
    AgentToolRegistry,
    RuntimeSkill,
    SemanticClosure,
    TemporalGraph,
    Maintenance,
    GovernanceJob,
    RetentionCompaction,
    TranscriptCommit,
    TranscriptAttributes,
    TranscriptLifecycle,
    MemorySpaceImport,
    Recover,
    Close,
    SubjectSoulEvidence,
    SubjectSoulProvision,
    SubjectSoulRevision,
    SubjectSoulArchive,
    SubjectSoulRestore,
    SubjectSoulReset,
    SubjectSoulReseed,
    SubjectSoulDelete,
    RelationshipSourceControl,
}

impl MemoryMutationSurface {
    pub const ALL: [Self; 28] = [
        Self::WriteWithOperation,
        Self::WriteWithoutOperation,
        Self::LongTermMutateWithOperation,
        Self::LongTermMutateWithoutOperation,
        Self::FinalizeTurn,
        Self::MemoryGovernancePolicy,
        Self::AgentToolRegistry,
        Self::RuntimeSkill,
        Self::SemanticClosure,
        Self::TemporalGraph,
        Self::Maintenance,
        Self::GovernanceJob,
        Self::RetentionCompaction,
        Self::TranscriptCommit,
        Self::TranscriptAttributes,
        Self::TranscriptLifecycle,
        Self::MemorySpaceImport,
        Self::Recover,
        Self::Close,
        Self::SubjectSoulEvidence,
        Self::SubjectSoulProvision,
        Self::SubjectSoulRevision,
        Self::SubjectSoulArchive,
        Self::SubjectSoulRestore,
        Self::SubjectSoulReset,
        Self::SubjectSoulReseed,
        Self::SubjectSoulDelete,
        Self::RelationshipSourceControl,
    ];

    pub const fn reliability(self) -> MemoryMutationReliability {
        match self {
            Self::WriteWithOperation
            | Self::LongTermMutateWithOperation
            | Self::SubjectSoulEvidence
            | Self::SubjectSoulProvision
            | Self::SubjectSoulRevision
            | Self::SubjectSoulArchive
            | Self::SubjectSoulRestore
            | Self::SubjectSoulReset
            | Self::SubjectSoulReseed
            | Self::SubjectSoulDelete
            | Self::RelationshipSourceControl => MemoryMutationReliability::DurableStoreReceipt,
            Self::FinalizeTurn | Self::GovernanceJob | Self::TranscriptCommit => {
                MemoryMutationReliability::DomainOwnedReceipt
            }
            Self::WriteWithoutOperation
            | Self::LongTermMutateWithoutOperation
            | Self::MemoryGovernancePolicy
            | Self::AgentToolRegistry
            | Self::RuntimeSkill
            | Self::SemanticClosure
            | Self::TemporalGraph
            | Self::Maintenance
            | Self::RetentionCompaction
            | Self::TranscriptAttributes
            | Self::TranscriptLifecycle
            | Self::MemorySpaceImport
            | Self::Recover
            | Self::Close => MemoryMutationReliability::ExplicitlyNonDurable,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryMutationReliability {
    DurableStoreReceipt,
    DomainOwnedReceipt,
    ExplicitlyNonDurable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationCapability {
    pub surface: MemoryMutationSurface,
    pub reliability: MemoryMutationReliability,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryMutationCapabilityCatalog {
    pub operations: Vec<MemoryMutationCapability>,
}

impl MemoryMutationCapabilityCatalog {
    pub fn current() -> Self {
        Self {
            operations: MemoryMutationSurface::ALL
                .into_iter()
                .map(|surface| MemoryMutationCapability {
                    surface,
                    reliability: surface.reliability(),
                })
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn public_sdk_mutation_inventory_is_complete_unique_and_explicit() {
        let catalog = MemoryMutationCapabilityCatalog::current();
        assert_eq!(catalog.operations.len(), MemoryMutationSurface::ALL.len());
        assert_eq!(
            catalog
                .operations
                .iter()
                .map(|item| item.surface)
                .collect::<HashSet<_>>()
                .len(),
            MemoryMutationSurface::ALL.len()
        );
        assert_eq!(
            catalog
                .operations
                .iter()
                .filter(|item| {
                    item.reliability == MemoryMutationReliability::DurableStoreReceipt
                })
                .count(),
            11
        );
        assert!(catalog.operations.iter().any(|item| {
            item.surface == MemoryMutationSurface::WriteWithoutOperation
                && item.reliability == MemoryMutationReliability::ExplicitlyNonDurable
        }));
        assert!(catalog.operations.iter().any(|item| {
            item.surface == MemoryMutationSurface::GovernanceJob
                && item.reliability == MemoryMutationReliability::DomainOwnedReceipt
        }));
    }
}
