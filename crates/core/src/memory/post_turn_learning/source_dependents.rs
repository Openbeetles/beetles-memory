//! Reverse discovery for actual governed sources, not a source ACL or a copy of
//! source content. Store validates both directions against retained producers.

use super::{
    domain_digest, evidence::identifier, ProceduralProducerBindingV1, ProceduralProducerScopeV1,
    ProceduralProducerSourceAuthorityV1, ProceduralSourcePostImageV1,
};
use crate::memory::{GovernedMemoryOwnerRef, MemoryMutationOperationIdentity};
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
            schema_version: 1,
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
        if self.dependents.len() >= MAX_PROCEDURAL_SOURCE_DEPENDENTS {
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
        if self.schema_version != 1
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
            || self.dependents.len() > MAX_PROCEDURAL_SOURCE_DEPENDENTS
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
        ))
        .map_err(|_| invalid())?;
        Ok(domain_digest("procedural_source_dependents_v1", &[&bytes]))
    }
}

fn invalid() -> Error {
    Error::config(
        "procedural_source_dependents",
        "source dependent root is not canonical",
    )
}
