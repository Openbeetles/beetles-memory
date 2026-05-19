use crate::{MemoryDomain, MemoryPlane, SourceRef, SubjectAssemblyReport};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum ProjectionSurface {
    Prompt,
    ToolContext,
    OperatorInspection,
    Adapter,
    Replay,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionBlock {
    pub record_id: String,
    pub domain: MemoryDomain,
    pub plane: MemoryPlane,
    pub content: String,
    pub source: SourceRef,
    pub privacy_filtered: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectionReport {
    pub surface: ProjectionSurface,
    pub blocks: Vec<ProjectionBlock>,
    pub privacy_filtered_count: usize,
    pub subject_assembly: Option<SubjectAssemblyReport>,
    pub warnings: Vec<String>,
}
