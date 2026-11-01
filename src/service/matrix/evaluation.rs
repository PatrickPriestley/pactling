//! Pure matrix reduction: no database, clock, HTTP or external calls.
use super::{Matrix, MatrixQuery, Notice, Row, Selector};
use crate::db::{
    environments::Environment,
    versions::{BranchVersion, Tag, Version},
};
use std::collections::HashMap;

/// Typed causes preserve provenance independently of compatibility wording.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    MissingSelector(Selector),
    IgnoreParticipantMissing(String),
    IgnoreVersionMissing {
        participant: String,
        version: String,
    },
    UnpinnedSelector,
    EnvironmentUnspecified,
    VerificationFailed,
    VerificationMissing,
    Verified,
    NoRequiredRows,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowOutcome {
    Ignored,
    Successful,
    Failed,
    Unverified,
}

#[derive(Clone, Debug)]
pub struct VersionDetails {
    pub branches: Vec<BranchVersion>,
    pub environments: Vec<Environment>,
    pub tags: Vec<(Tag, bool)>,
}

/// Owned input, constructed only by resolution under one read snapshot.
#[derive(Clone, Debug)]
pub struct ResolvedMatrix {
    pub(super) query: MatrixQuery,
    pub(super) rows: Vec<Row>,
    pub(super) notices: Vec<Notice>,
    pub(super) versions: HashMap<i64, VersionDetails>,
}

/// One authoritative decision plus everything its compatibility view needs.
#[derive(Clone, Debug)]
pub struct DecisionEvaluation {
    query: MatrixQuery,
    matrix: Matrix,
    versions: HashMap<i64, VersionDetails>,
    row_outcomes: Vec<RowOutcome>,
}
impl DecisionEvaluation {
    pub fn query(&self) -> &MatrixQuery {
        &self.query
    }
    pub fn matrix(&self) -> &Matrix {
        &self.matrix
    }
    pub fn into_matrix(self) -> Matrix {
        self.matrix
    }
    pub fn version_details(&self, version: &Version) -> &VersionDetails {
        &self.versions[&version.id]
    }
    /// Same order as matrix.rows, computed in the same pass as its counts/verdict.
    pub fn row_outcomes(&self) -> &[RowOutcome] {
        &self.row_outcomes
    }
}

pub fn evaluate(resolved: ResolvedMatrix) -> DecisionEvaluation {
    let ResolvedMatrix {
        query: q,
        mut rows,
        mut notices,
        versions,
    } = resolved;
    // Verified in Ruby's DeploymentStatusSummary#reasons: warnings (ignore warnings in
    // request order, then the environment and version-number warnings) precede errors,
    // and within each kind the request order is kept.
    if q.selectors.iter().any(|s| !s.is_name_only())
        && q.selectors.iter().all(|s| s.version.is_none())
    {
        notices.push(Notice { cause: Cause::UnpinnedSelector, kind: "warning", text: "WARN: It is recommended to specify the version number (rather than the tag or branch) of the pacticipant you wish to deploy to avoid race conditions. Without a version number, this result will not be reliable.".into() });
    }
    let (warnings, errors): (Vec<Notice>, Vec<Notice>) =
        notices.into_iter().partition(|n| n.kind == "warning");
    let mut notices: Vec<Notice> = vec![];
    for n in warnings.into_iter().chain(errors) {
        if !notices.iter().any(|seen| seen.text == n.text) {
            notices.push(n);
        }
    }
    // Overlapping selectors can select the same evidence more than once.
    let mut seen = std::collections::HashSet::new();
    rows.retain(|r| {
        seen.insert((
            r.pact.id,
            r.provider.version.as_ref().map(|v| v.id),
            r.provider.description.clone(),
        ))
    });
    let mut rows = apply_latestby(rows, q.latestby.as_deref());
    for row in &mut rows {
        row.ignored |= is_ignored(
            &q,
            &row.consumer.pacticipant.name,
            row.consumer.version.as_ref(),
        ) || is_ignored(
            &q,
            &row.provider.pacticipant.name,
            row.provider.version.as_ref(),
        );
    }
    let (mut result, row_outcomes) = summarize(rows, notices);
    // The Ruby document includes ignored=0 when an ignore was requested.
    result.has_ignores = !q.ignore.is_empty();
    DecisionEvaluation {
        query: q,
        matrix: result,
        versions,
        row_outcomes,
    }
}

/// Ruby Matrix::Repository#apply_latestby runs after selection and before ignores.
pub(super) fn apply_latestby(mut rows: Vec<Row>, latestby: Option<&str>) -> Vec<Row> {
    let (by_consumer_version, by_provider_version) = match latestby {
        Some("cvpv") => (true, true),
        Some("cvp") => (true, false),
        Some("cp") => (false, false),
        _ => return rows,
    };
    // Reproduce Ruby's query order before grouping: it determines the first null
    // row and, for equal provider orders, the last row in the stable sort.
    rows.sort_by(|a, b| matrix_query_order(b).cmp(&matrix_query_order(a)));
    let mut groups = std::collections::HashMap::new();
    let mut selected: Vec<Row> = vec![];
    for row in rows {
        let key = (
            row.consumer.pacticipant.id,
            by_consumer_version
                .then(|| row.consumer.version.as_ref().map(|v| v.id))
                .flatten(),
            row.provider.pacticipant.id,
            by_provider_version
                .then(|| row.provider.version.as_ref().map(|v| v.id))
                .flatten(),
        );
        match groups.entry(key) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(selected.len());
                selected.push(row);
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                let previous = &mut selected[*entry.get()];
                // A group's first null provider row is retained. Otherwise Ruby's
                // stable sort takes the last maximum provider creation order.
                if previous.provider.version.is_some()
                    && row.provider.version.as_ref().map(|v| v.id)
                        >= previous.provider.version.as_ref().map(|v| v.id)
                {
                    *previous = row;
                }
            }
        }
    }
    selected
}

fn matrix_query_order(row: &Row) -> (&str, i64, Option<i64>) {
    let consumer_created = row
        .consumer
        .version
        .as_ref()
        .map_or("", |v| v.created_at.as_str());
    // Ruby's provider_version_created_at query alias comes from the latest
    // verification cache, whose created_at is the verification's timestamp.
    let verification_created = row
        .verification
        .as_ref()
        .map_or("", |v| v.created_at.as_str());
    (
        consumer_created.max(verification_created),
        row.pact.id,
        row.verification.as_ref().map(|v| v.id),
    )
}

pub(super) fn is_ignored(q: &MatrixQuery, name: &str, version: Option<&Version>) -> bool {
    q.ignore.iter().any(|(p, v)| {
        p == name
            && v.as_ref()
                .is_none_or(|number| version.is_some_and(|actual| &actual.number == number))
    })
}

fn summarize(mut rows: Vec<Row>, mut notices: Vec<Notice>) -> (Matrix, Vec<RowOutcome>) {
    let missing_version = notices
        .iter()
        .any(|n| matches!(n.cause, Cause::MissingSelector(_)));
    rows.sort_by(|a, b| {
        a.ignored.cmp(&b.ignored).then_with(|| {
            a.consumer
                .pacticipant
                .name
                .cmp(&b.consumer.pacticipant.name)
                .then(
                    a.provider
                        .pacticipant
                        .name
                        .cmp(&b.provider.pacticipant.name),
                )
                .then(
                    b.consumer
                        .version
                        .as_ref()
                        .map(|v| v.id)
                        .cmp(&a.consumer.version.as_ref().map(|v| v.id)),
                )
                .then(
                    b.provider
                        .version
                        .as_ref()
                        .map(|v| v.id)
                        .cmp(&a.provider.version.as_ref().map(|v| v.id)),
                )
        })
    });

    let (mut success, mut failed, mut unknown, mut ignored) = (0, 0, 0, 0);
    let (mut failures, mut unknowns) = (vec![], vec![]);
    let mut row_outcomes = Vec::with_capacity(rows.len());
    for r in &rows {
        if r.ignored {
            row_outcomes.push(RowOutcome::Ignored);
            ignored += 1;
            continue;
        }
        match &r.verification {
            Some(v) if v.success => {
                row_outcomes.push(RowOutcome::Successful);
                success += 1;
            }
            Some(_) => {
                row_outcomes.push(RowOutcome::Failed);
                failed += 1;
                failures.push(Notice {
                    cause: Cause::VerificationFailed,
                    kind: "error",
                    text: format!(
                        "The verification for the pact between {} and {} failed",
                        r.consumer.description, r.provider.description
                    ),
                });
            }
            None => {
                row_outcomes.push(RowOutcome::Unverified);
                unknown += 1;
                unknowns.push(Notice {
                    cause: Cause::VerificationMissing,
                    kind: "error",
                    text: format!(
                        "There is no verified pact between {} and {}",
                        r.consumer.description, r.provider.description
                    ),
                });
            }
        }
    }
    // Verified in Ruby's DeploymentStatusSummary: a missing selector version wins
    // (false, and only its message); then any unknown row makes the verdict null; then
    // a failure makes it false. Failure messages precede unverified messages.
    if !missing_version {
        notices.extend(failures);
        notices.extend(unknowns);
    }
    let deployable = if missing_version {
        Some(false)
    } else if unknown > 0 {
        None
    } else if failed > 0 {
        Some(false)
    } else {
        if success > 0 {
            notices.push(Notice {
                cause: Cause::Verified,
                kind: "success",
                text: "All required verification results are published and successful".into(),
            });
        } else {
            notices.push(Notice {
                cause: Cause::NoRequiredRows,
                kind: "success",
                text: "There are no missing dependencies".into(),
            });
        }
        Some(true)
    };
    (
        Matrix {
            rows,
            deployable,
            notices,
            success,
            failed,
            unknown,
            ignored,
            has_ignores: false,
        },
        row_outcomes,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_selector_is_a_typed_cause_not_a_notice_kind_or_text() {
        let input = |cause, kind| ResolvedMatrix {
            query: MatrixQuery::default(),
            rows: vec![],
            notices: vec![Notice {
                cause,
                kind,
                text: "arbitrary presentation text".into(),
            }],
            versions: HashMap::new(),
        };
        let selector = Selector {
            pacticipant: "C".into(),
            version: Some("missing".into()),
            ..Default::default()
        };
        assert_eq!(
            evaluate(input(Cause::MissingSelector(selector), "warning"))
                .matrix()
                .deployable,
            Some(false)
        );
        assert_eq!(
            evaluate(input(Cause::EnvironmentUnspecified, "error"))
                .matrix()
                .deployable,
            Some(true)
        );
    }
}
