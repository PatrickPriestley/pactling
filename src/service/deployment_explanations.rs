//! Pure explanatory projection of the authoritative matrix decision.
use super::matrix::{Selector, evaluation::DecisionEvaluation};
use serde_json::Value;

#[derive(Debug)]
pub struct Explanation {
    pub decision: &'static str,
    pub completeness: &'static str,
    pub summary: String,
    pub reasons: Vec<Reason>,
    pub actions: Vec<Action>,
}
#[derive(Debug)]
pub struct Reason {
    pub code: &'static str,
    pub role: &'static str,
    pub message: String,
    pub evidence_refs: Vec<String>,
    pub action_ids: Vec<String>,
}
#[derive(Debug)]
pub struct Action {
    pub id: String,
    pub kind: &'static str,
    pub message: String,
    pub evidence_refs: Vec<String>,
    pub target: Value,
    pub row_index: Option<usize>,
}

use super::matrix::{
    evaluation::{Cause, RowOutcome},
    provenance::ExclusionKind,
};
use serde_json::json;

pub fn selector_document(selector: &Selector) -> Value {
    json!({"pacticipant":selector.pacticipant,"version":selector.version,"latest":selector.latest,
        "tag":selector.tag,"branch":selector.branch,"mainBranch":selector.main_branch})
}

impl Explanation {
    fn reason(
        &mut self,
        code: &'static str,
        role: &'static str,
        message: String,
        evidence_refs: Vec<String>,
        action_ids: Vec<String>,
    ) {
        self.reasons.push(Reason {
            code,
            role,
            message,
            evidence_refs,
            action_ids,
        });
    }
    fn action(
        &mut self,
        kind: &'static str,
        message: String,
        evidence_refs: &[String],
        target: Value,
        row_index: Option<usize>,
    ) -> String {
        if let Some(existing) = self
            .actions
            .iter_mut()
            .find(|action| (action.kind, &action.target) == (kind, &target))
        {
            existing.evidence_refs.extend_from_slice(evidence_refs);
            existing.evidence_refs.sort();
            existing.evidence_refs.dedup();
            return existing.id.clone();
        }
        let id = format!("action-{}", self.actions.len() + 1);
        self.actions.push(Action {
            id: id.clone(),
            kind,
            message,
            evidence_refs: evidence_refs.to_vec(),
            target,
            row_index,
        });
        id
    }
}

pub fn explain(evaluation: &DecisionEvaluation) -> Explanation {
    let matrix = evaluation.matrix();
    let provenance = evaluation.provenance();
    let query = evaluation.query();
    let missing = matrix
        .notices
        .iter()
        .any(|notice| matches!(notice.cause, Cause::MissingSelector(_)));
    let mut explanation = Explanation {
        decision: match matrix.deployable {
            Some(true) => "allowed",
            Some(false) => "blocked",
            None => "unknown",
        },
        completeness: if !missing && matrix.unknown == 0 {
            "complete"
        } else {
            "incomplete"
        },
        summary: String::new(),
        reasons: vec![],
        actions: vec![],
    };
    for notice in &matrix.notices {
        match &notice.cause {
            Cause::MissingSelector(selector) => {
                for resolved in provenance
                    .selectors
                    .iter()
                    .filter(|resolved| resolved.requested == *selector)
                {
                    let refs = vec![format!("/resolution/selectors/{}", resolved.index)];
                    let id=explanation.action("RESOLVE_SELECTOR", "Correct this selector or publish and record its intended version and required contract results; creating a version alone does not establish compatibility.".into(),&refs,json!({"selectorIndex":resolved.index,"requested":selector_document(selector)}),None);
                    explanation.reason(
                        "SELECTOR_VERSION_MISSING",
                        "decisive",
                        format!(
                            "The requested selector for {} resolves to no versions.",
                            selector.pacticipant
                        ),
                        refs,
                        vec![id],
                    );
                }
            }
            Cause::IgnoreParticipantMissing(_) | Cause::IgnoreVersionMissing { .. } => {
                for (index, (participant, version)) in
                    query
                        .ignore
                        .iter()
                        .enumerate()
                        .filter(|(_, (participant, version))| match &notice.cause {
                            Cause::IgnoreParticipantMissing(name) => participant == name,
                            Cause::IgnoreVersionMissing {
                                participant: name,
                                version: number,
                            } => participant == name && version.as_ref() == Some(number),
                            _ => false,
                        })
                {
                    let refs = vec![format!("/resolution/ignores/{index}")];
                    let id = explanation.action(
                        "CORRECT_IGNORE_REQUEST",
                        "Correct this exact ignore request without broadening it.".into(),
                        &refs,
                        json!({"ignoreIndex":index,"pacticipant":participant,"version":version}),
                        None,
                    );
                    explanation.reason(
                        "IGNORE_TARGET_NOT_FOUND",
                        "advisory",
                        notice.text.clone(),
                        refs,
                        vec![id],
                    );
                }
            }
            _ => {}
        }
    }
    for (index, (row, outcome)) in matrix
        .rows
        .iter()
        .zip(evaluation.row_outcomes())
        .enumerate()
    {
        let origin = &provenance.rows[index];
        let mut refs = vec![
            format!("/rows/{index}/contract"),
            format!("/rows/{index}/verification"),
        ];
        refs.extend(
            origin
                .candidate_indices
                .iter()
                .map(|i| format!("/resolution/candidates/{i}")),
        );
        match outcome {
            RowOutcome::Ignored => {
                refs.extend(
                    origin
                        .ignore_indices
                        .iter()
                        .map(|i| format!("/resolution/ignores/{i}")),
                );
                explanation.reason(
                    "ROW_IGNORED",
                    "advisory",
                    "This row is excluded from the verdict by the matched ignore request.".into(),
                    refs,
                    vec![],
                );
            }
            RowOutcome::Unverified => {
                let candidate = row.provider_candidate.as_ref();
                let mut target = json!({"consumer":row.consumer.pacticipant.name,"consumerVersion":row.consumer.version.as_ref().map(|v|&v.number),"provider":row.provider.pacticipant.name,"providerVersion":candidate.map(|v|&v.number),"contractSha":row.pact.sha});
                let (kind, message, row_index) = match candidate {
                    Some(version) => (
                        "VERIFY_CONTRACT",
                        format!(
                            "Verify this exact contract with {} version {} and publish the result.",
                            row.provider.pacticipant.name, version.number
                        ),
                        Some(index),
                    ),
                    None => {
                        target["environment"] = json!(query.environment);
                        target["tag"] = json!(query.tag);
                        ("RESOLVE_TARGET_VERSION","Determine and record the intended provider version for this target before verifying this contract.".into(),None)
                    }
                };
                let id =
                    explanation.action(kind, message.clone(), &refs, target.clone(), row_index);
                explanation.reason("VERIFICATION_MISSING",if missing {"supporting"}else{"decisive"},format!("No verification is recorded for this exact {} to {} contract and selected provider version.",row.consumer.pacticipant.name,row.provider.pacticipant.name),refs.clone(),vec![id]);
                if candidate.is_none() {
                    let id = explanation.action(kind, message, &refs, target, row_index);
                    explanation.reason(
                        "TARGET_VERSION_UNRESOLVED",
                        "supporting",
                        "No exact provider version was resolved for this contract's target.".into(),
                        refs,
                        vec![id],
                    );
                }
            }
            RowOutcome::Failed => {
                let version = row
                    .provider
                    .version
                    .as_ref()
                    .expect("failed verification has provider version");
                let target = json!({"consumer":row.consumer.pacticipant.name,"consumerVersion":row.consumer.version.as_ref().map(|v|&v.number),"provider":row.provider.pacticipant.name,"providerVersion":version.number,"contractSha":row.pact.sha});
                let id=explanation.action("FIX_FAILED_VERIFICATION","Inspect this result, fix the incompatibility, and publish a new verification for this exact contract and provider version.".into(),&refs,target,Some(index));
                explanation.reason(
                    "VERIFICATION_FAILED",
                    if !missing && matrix.unknown == 0 {
                        "decisive"
                    } else {
                        "supporting"
                    },
                    format!(
                        "{} version {} failed verification of this {} contract.",
                        row.provider.pacticipant.name,
                        version.number,
                        row.consumer.pacticipant.name
                    ),
                    refs,
                    vec![id],
                );
            }
            RowOutcome::Successful => {}
        }
    }
    if matrix.deployable == Some(true) {
        let (code, message) = if matrix.success > 0 {
            (
                "ALL_REQUIRED_VERIFICATIONS_SUCCESSFUL",
                "All considered contract verifications succeeded.",
            )
        } else {
            (
                "NO_RECORDED_REQUIRED_ROWS",
                "No recorded rows remain required by this query.",
            )
        };
        let refs = evaluation
            .row_outcomes()
            .iter()
            .enumerate()
            .filter(|(_, outcome)| **outcome == RowOutcome::Successful)
            .map(|(index, _)| format!("/rows/{index}/verification"))
            .collect();
        explanation.reason(code, "decisive", message.into(), refs, vec![]);
    }
    for (index, excluded) in provenance.excluded.iter().enumerate() {
        let (code, message) = match excluded.kind {
            ExclusionKind::Latestby => (
                "LATESTBY_EXCLUDED",
                "This candidate row was removed by latestby selection.",
            ),
            ExclusionKind::Duplicate => (
                "RESOLUTION_EXCLUDED",
                "Overlapping selectors contributed duplicate evidence.",
            ),
            ExclusionKind::UnverifiedCandidate => (
                "RESOLUTION_EXCLUDED",
                "Resolution retained other evidence instead of this unverified candidate.",
            ),
        };
        let mut refs = vec![format!("/resolution/excluded/{index}")];
        refs.extend(excluded.selected_row.map(|index| format!("/rows/{index}")));
        explanation.reason(code, "advisory", message.into(), refs, vec![]);
    }
    explanation.summary = explanation
        .reasons
        .iter()
        .filter(|reason| reason.role == "decisive")
        .map(|reason| reason.message.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    explanation
}
