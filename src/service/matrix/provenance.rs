//! Owned resolution evidence. Candidate identities remain separate from Ruby's null rows.
use super::{Row, Selector};
use crate::db::{
    environments::{DeployedVersion, Environment, ReleasedVersion},
    versions::Version,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct EvaluationContext {
    pub id: String,
    pub started_at: String,
    pub evaluated_at: String,
}
#[derive(Clone, Debug)]
pub struct SelectorResolution {
    pub index: usize,
    pub requested: Selector,
    pub versions: Vec<Version>,
    pub missing: bool,
}
#[derive(Clone, Debug)]
pub enum MembershipSource {
    Deployed(DeployedVersion),
    Released(ReleasedVersion),
}
#[derive(Clone, Debug)]
pub struct Candidate {
    pub version: Version,
    pub selector_indices: Vec<usize>,
    pub sources: Vec<MembershipSource>,
}
#[derive(Clone, Debug, Default)]
pub struct RowProvenance {
    pub candidate_indices: Vec<usize>,
    pub selector_indices: Vec<usize>,
    pub ignore_indices: Vec<usize>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExclusionKind {
    UnverifiedCandidate,
    Duplicate,
    Latestby,
}
#[derive(Clone, Debug)]
pub struct ExcludedEvidence {
    pub kind: ExclusionKind,
    pub row: Row,
    pub candidate_indices: Vec<usize>,
    pub selector_indices: Vec<usize>,
    pub selected_row: Option<usize>,
}
#[derive(Clone, Debug, Default)]
pub struct Provenance {
    pub selectors: Vec<SelectorResolution>,
    pub environment: Option<Environment>,
    pub candidates: Vec<Candidate>,
    pub rows: Vec<RowProvenance>,
    pub excluded: Vec<ExcludedEvidence>,
}

/// Merge request origins without changing their original request order.
pub(super) fn merge_indices(target: &mut Vec<usize>, source: &[usize]) {
    target.extend_from_slice(source);
    target.sort_unstable();
    target.dedup();
}
