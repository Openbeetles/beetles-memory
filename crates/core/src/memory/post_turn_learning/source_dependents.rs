//! Reverse discovery for actual governed sources, not a source ACL or a copy of
//! source content. Store validates both directions against retained producers.

use super::{
    domain_digest, evidence::identifier, ProceduralProducerBindingV1, ProceduralProducerScopeV1,
    ProceduralProducerSourceAuthorityV1, ProceduralSourcePostImageV1,
};
use crate::memory::{
    GovernedMemoryOwnerRef, GovernedOwnerRevisionRef, MemoryMutationOperationIdentity,
};
use crate::{Error, Result};
use serde::{Deserialize, Serialize};

pub const MAX_PROCEDURAL_SOURCE_DEPENDENTS: usize = 256;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralSourceTransitionV1 {
    pub owner_ref: GovernedMemoryOwnerRef,
    pub before: ProceduralSourcePostImageV1,
    pub after: ProceduralSourcePostImageV1,
}

impl ProceduralSourceTransitionV1 {
    pub fn validate(&self) -> Result<()> {
        if ProceduralSourceDependentsRootV1::transition_valid(
            &self.before,
            &self.after,
            &self.owner_ref,
        ) {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralSourceDependentV1 {
    pub binding_key: String,
    pub scope: ProceduralProducerScopeV1,
}

/// An exact first-producing method commitment. The source root retains no raw
/// method or source text, and tombstones can preserve this commitment safely.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralMethodSourceDependentV1 {
    pub scope: super::ProceduralSubjectScopeV1,
    pub owner_revision: GovernedOwnerRevisionRef,
    pub content_digest: String,
    pub source_job_id: String,
    pub source_revision: GovernedOwnerRevisionRef,
}

impl ProceduralMethodSourceDependentV1 {
    pub fn same_identity(&self, other: &Self) -> bool {
        self.scope == other.scope
            && self.owner_revision.owner_ref == other.owner_revision.owner_ref
            && self.source_job_id == other.source_job_id
            && self.source_revision == other.source_revision
    }

    pub fn validates_source(&self, space: &str, owner: &GovernedMemoryOwnerRef) -> bool {
        self.scope.validate_contract()
            && self.scope.memory_space_id == space
            && self.owner_revision.is_valid()
            && self.owner_revision.owner_ref.owner_plane
                == crate::memory::GovernedMemoryOwnerPlane::AgentToolExperience
            && super::is_digest(&self.content_digest)
            && identifier(&self.source_job_id)
            && self.source_revision.is_valid()
            && self.source_revision.owner_ref == *owner
    }

    fn ordering_key(
        &self,
    ) -> (
        &str,
        &GovernedMemoryOwnerRef,
        &str,
        &GovernedOwnerRevisionRef,
    ) {
        (
            &self.scope.mounted_subject_id,
            &self.owner_revision.owner_ref,
            &self.source_job_id,
            &self.source_revision,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralSourceChangeCommitmentV1 {
    pub transaction_id: String,
    pub operation: Option<MemoryMutationOperationIdentity>,
    pub before: Option<ProceduralSourcePostImageV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProceduralSourceDependentsRootV1 {
    pub schema_version: u32,
    pub physical_key: String,
    pub memory_space_id: String,
    pub owner_ref: GovernedMemoryOwnerRef,
    pub revision: u64,
    pub current: ProceduralSourcePostImageV1,
    pub last_change: ProceduralSourceChangeCommitmentV1,
    pub dependents: Vec<ProceduralSourceDependentV1>,
    pub method_dependents: Vec<ProceduralMethodSourceDependentV1>,
    pub content_digest: String,
}

impl ProceduralSourceDependentsRootV1 {
    pub fn key(space: &str, owner: &GovernedMemoryOwnerRef) -> Result<String> {
        if !identifier(space) || !ProceduralSourcePostImageV1::Deleted.validates_owner(owner) {
            return Err(invalid());
        }
        Ok(format!(
            "procedural_source_dependents:{}",
            domain_digest(
                "procedural_source_dependents_key_v1",
                &[
                    space.as_bytes(),
                    &serde_json::to_vec(owner).map_err(|_| invalid())?
                ]
            )
        ))
    }

    pub fn create(
        space: String,
        owner: GovernedMemoryOwnerRef,
        current: ProceduralSourcePostImageV1,
        transaction_id: String,
        operation: Option<MemoryMutationOperationIdentity>,
    ) -> Result<Self> {
        if !matches!(current, ProceduralSourcePostImageV1::Present { .. }) {
            return Err(invalid());
        }
        let mut root = Self {
            schema_version: 2,
            physical_key: Self::key(&space, &owner)?,
            memory_space_id: space,
            owner_ref: owner,
            revision: 1,
            current,
            last_change: ProceduralSourceChangeCommitmentV1 {
                transaction_id,
                operation,
                before: None,
            },
            dependents: Vec::new(),
            method_dependents: Vec::new(),
            content_digest: String::new(),
        };
        root.seal()?;
        Ok(root)
    }

    pub fn bind(&self, producer: &ProceduralProducerBindingV1) -> Result<Self> {
        self.validate()?;
        if !producer.validate_contract()
            || producer.spec.scope.memory_space_id != self.memory_space_id
            || !matches!(&producer.spec.source_authority, ProceduralProducerSourceAuthorityV1::GovernedSource { source_revision }
                if source_revision.owner_ref == self.owner_ref)
        {
            return Err(invalid());
        }
        let reference = ProceduralSourceDependentV1 {
            binding_key: producer.spec.binding_key()?,
            scope: producer.spec.scope.clone(),
        };
        if let Some(existing) = self
            .dependents
            .iter()
            .find(|item| item.binding_key == reference.binding_key)
        {
            return if *existing == reference {
                Ok(self.clone())
            } else {
                Err(invalid())
            };
        }
        if self.dependents.len() + self.method_dependents.len() >= MAX_PROCEDURAL_SOURCE_DEPENDENTS
        {
            return Err(Error::config(
                "procedural_source_dependents",
                "source dependent retention capacity is exhausted",
            ));
        }
        let mut next = self.clone();
        next.revision = next.revision.checked_add(1).ok_or_else(invalid)?;
        next.dependents.push(reference);
        next.dependents
            .sort_by(|a, b| a.binding_key.cmp(&b.binding_key));
        next.seal()?;
        Ok(next)
    }

    pub fn bind_method(&self, reference: ProceduralMethodSourceDependentV1) -> Result<Self> {
        self.validate()?;
        if !reference.validates_source(&self.memory_space_id, &self.owner_ref) {
            return Err(invalid());
        }
        if let Some(existing) = self
            .method_dependents
            .iter()
            .find(|item| item.same_identity(&reference))
        {
            return if *existing == reference {
                Ok(self.clone())
            } else {
                Err(invalid())
            };
        }
        if !matches!(&self.current, ProceduralSourcePostImageV1::Present { revision, .. }
            if revision.owner_revision >= reference.source_revision.owner_revision)
        {
            return Err(invalid());
        }
        if self.dependents.len() + self.method_dependents.len() >= MAX_PROCEDURAL_SOURCE_DEPENDENTS
        {
            return Err(Error::config(
                "procedural_source_dependents",
                "source dependent retention capacity is exhausted",
            ));
        }
        let mut next = self.clone();
        next.revision = next.revision.checked_add(1).ok_or_else(invalid)?;
        next.method_dependents.push(reference);
        next.method_dependents
            .sort_by(|a, b| a.ordering_key().cmp(&b.ordering_key()));
        next.seal()?;
        Ok(next)
    }

    pub fn dependent_subjects(&self) -> std::collections::BTreeSet<String> {
        self.dependents
            .iter()
            .map(|item| item.scope.mounted_subject_id.clone())
            .chain(
                self.method_dependents
                    .iter()
                    .map(|item| item.scope.mounted_subject_id.clone()),
            )
            .collect()
    }

    pub fn advance(
        &self,
        current: ProceduralSourcePostImageV1,
        transaction_id: String,
        operation: Option<MemoryMutationOperationIdentity>,
    ) -> Result<Self> {
        self.validate()?;
        if current == self.current {
            return Ok(self.clone());
        }
        if !Self::transition_valid(&self.current, &current, &self.owner_ref) {
            return Err(invalid());
        }
        let mut next = self.clone();
        next.revision = next.revision.checked_add(1).ok_or_else(invalid)?;
        next.last_change = ProceduralSourceChangeCommitmentV1 {
            transaction_id,
            operation,
            before: Some(self.current.clone()),
        };
        next.current = current;
        next.seal()?;
        Ok(next)
    }

    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 2
            || self.revision == 0
            || self.physical_key != Self::key(&self.memory_space_id, &self.owner_ref)?
            || !self.current.validates_owner(&self.owner_ref)
            || !identifier(&self.last_change.transaction_id)
            || self.last_change.operation.as_ref().is_some_and(|op| {
                op.validate_contract().is_err() || op.memory_space_id() != self.memory_space_id
            })
            || match &self.last_change.before {
                None => !matches!(self.current, ProceduralSourcePostImageV1::Present { .. }),
                Some(before) => !Self::transition_valid(before, &self.current, &self.owner_ref),
            }
            || self.dependents.len() + self.method_dependents.len()
                > MAX_PROCEDURAL_SOURCE_DEPENDENTS
            || self
                .method_dependents
                .iter()
                .any(|item| !item.validates_source(&self.memory_space_id, &self.owner_ref))
            || self
                .method_dependents
                .windows(2)
                .any(|pair| pair[0].ordering_key() >= pair[1].ordering_key())
            || self.dependents.iter().any(|item| {
                !identifier(&item.binding_key)
                    || !item.scope.validate_contract()
                    || item.scope.memory_space_id != self.memory_space_id
            })
            || self
                .dependents
                .windows(2)
                .any(|pair| pair[0].binding_key >= pair[1].binding_key)
            || self.content_digest != self.digest()?
        {
            return Err(invalid());
        }
        Ok(())
    }

    fn transition_valid(
        before: &ProceduralSourcePostImageV1,
        after: &ProceduralSourcePostImageV1,
        owner: &GovernedMemoryOwnerRef,
    ) -> bool {
        if !before.validates_owner(owner) || !after.validates_owner(owner) {
            return false;
        }
        match (before, after) {
            (
                ProceduralSourcePostImageV1::Present { .. }
                | ProceduralSourcePostImageV1::Terminated { .. },
                ProceduralSourcePostImageV1::Deleted,
            ) => true,
            (
                ProceduralSourcePostImageV1::Present { revision: a, .. },
                ProceduralSourcePostImageV1::Present { revision: b, .. },
            ) => b.owner_revision > a.owner_revision,
            (
                ProceduralSourcePostImageV1::Present {
                    revision: a,
                    content_digest: da,
                },
                ProceduralSourcePostImageV1::Terminated {
                    revision: b,
                    content_digest: db,
                    ..
                },
            ) => a == b && da == db,
            _ => false,
        }
    }

    fn seal(&mut self) -> Result<()> {
        self.content_digest = self.digest()?;
        self.validate()
    }
    fn digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(&(
            self.schema_version,
            &self.physical_key,
            &self.memory_space_id,
            &self.owner_ref,
            self.revision,
            &self.current,
            &self.last_change,
            &self.dependents,
            &self.method_dependents,
        ))
        .map_err(|_| invalid())?;
        Ok(domain_digest("procedural_source_dependents_v2", &[&bytes]))
    }
}

fn invalid() -> Error {
    Error::config(
        "procedural_source_dependents",
        "source dependent root is not canonical",
    )
}

#[cfg(test)]
mod method_dependency_tests {
    use super::*;

    #[test]
    fn source_root_commits_explicit_method_dependencies() {
        let owner = GovernedMemoryOwnerRef::new(
            crate::memory::GovernedMemoryOwnerPlane::LongTerm,
            "source",
        );
        let root = ProceduralSourceDependentsRootV1::create(
            "space".into(),
            owner.clone(),
            ProceduralSourcePostImageV1::Present {
                revision: crate::memory::GovernedOwnerRevisionRef::try_new(owner, 1).unwrap(),
                content_digest: format!("sha256:{}", "a".repeat(64)),
            },
            "source-create".into(),
            None,
        )
        .unwrap();
        let mut value = serde_json::to_value(&root).unwrap();
        assert_eq!(
            value.get("method_dependents"),
            Some(&serde_json::json!([])),
            "a source root must explicitly commit even the empty method dependency set"
        );
        value.as_object_mut().unwrap().remove("method_dependents");
        assert!(serde_json::from_value::<ProceduralSourceDependentsRootV1>(value).is_err());
    }
    #[test]
    fn method_dependency_survives_source_withdrawal_and_cannot_be_rebound() {
        let owner = GovernedMemoryOwnerRef::new(
            crate::memory::GovernedMemoryOwnerPlane::LongTerm,
            "source",
        );
        let source_revision = GovernedOwnerRevisionRef::try_new(owner.clone(), 1).unwrap();
        let root = ProceduralSourceDependentsRootV1::create(
            "space".into(),
            owner,
            ProceduralSourcePostImageV1::Present {
                revision: source_revision.clone(),
                content_digest: format!("sha256:{}", "a".repeat(64)),
            },
            "create".into(),
            None,
        )
        .unwrap();
        let edge = ProceduralMethodSourceDependentV1 {
            scope: super::super::ProceduralSubjectScopeV1 {
                memory_space_id: "space".into(),
                mounted_subject_id: "agent-a".into(),
            },
            owner_revision: GovernedOwnerRevisionRef::try_new(
                GovernedMemoryOwnerRef::new(
                    crate::memory::GovernedMemoryOwnerPlane::AgentToolExperience,
                    "method",
                ),
                1,
            )
            .unwrap(),
            content_digest: format!("sha256:{}", "b".repeat(64)),
            source_job_id: "semantic-job".into(),
            source_revision,
        };
        let bound = root.bind_method(edge.clone()).unwrap();
        assert_eq!(bound.bind_method(edge.clone()).unwrap(), bound);
        assert_ne!(bound.content_digest, root.content_digest);
        assert_eq!(
            bound.dependent_subjects(),
            ["agent-a".into()].into_iter().collect()
        );
        let withdrawn = bound
            .advance(ProceduralSourcePostImageV1::Deleted, "delete".into(), None)
            .unwrap();
        assert_eq!(withdrawn.method_dependents, vec![edge.clone()]);
        assert_eq!(withdrawn.dependent_subjects(), bound.dependent_subjects());
        let mut late = edge.clone();
        late.source_job_id = "late-job".into();
        assert!(withdrawn.bind_method(late).is_err());
        let mut forged = edge.clone();
        forged.content_digest = format!("sha256:{}", "c".repeat(64));
        assert!(bound.bind_method(forged).is_err());
        let mut foreign = edge;
        foreign.scope.memory_space_id = "foreign".into();
        assert!(bound.bind_method(foreign).is_err());
        let mut duplicate = bound.clone();
        duplicate
            .method_dependents
            .push(duplicate.method_dependents[0].clone());
        assert!(duplicate.validate().is_err());
    }
}
