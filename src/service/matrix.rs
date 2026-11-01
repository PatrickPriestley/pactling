//! The matrix and can-i-deploy: query grammar, row resolution, verdict and the reason
//! strings the CLI prints. Strings and rules follow the Phase 4 capture in the plan.
pub mod evaluation;
pub mod provenance;
#[cfg(test)]
use evaluation::apply_latestby;
use evaluation::{Cause, DecisionEvaluation, ResolvedMatrix, VersionDetails};
use serde_json::json;

use crate::db::{
    Db, environments::Environment, pacticipants::Pacticipant, pacts::PactPublication,
    verifications::Verification, versions::Version,
};
use crate::error::{Error, Result};

/// One `q[]` group of the matrix query.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selector {
    pub pacticipant: String,
    pub version: Option<String>,
    pub latest: bool,
    pub tag: Option<String>,
    pub branch: Option<String>,
    pub main_branch: bool,
}

impl Selector {
    /// Only the pacticipant name was given: "all versions" in Ruby's resolver.
    pub fn is_name_only(&self) -> bool {
        self.version.is_none()
            && !self.latest
            && self.tag.is_none()
            && self.branch.is_none()
            && !self.main_branch
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MatrixQuery {
    pub selectors: Vec<Selector>,
    pub latestby: Option<String>,
    pub environment: Option<String>,
    /// `latest=true`: the other side is its latest version (optionally with `tag`).
    pub latest: bool,
    pub tag: Option<String>,
    pub ignore: Vec<(String, Option<String>)>,
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(b) = u8::from_str_radix(hex, 16)
        {
            out.push(b);
            i += 3;
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

impl MatrixQuery {
    /// Parses the raw query string. `q[][pacticipant]` starts a selector; the other `q[]`
    /// keys attach to the most recent one. Keys arrive percent-encoded or bare.
    pub fn parse(raw: Option<&str>) -> MatrixQuery {
        let mut q = MatrixQuery::default();
        for pair in raw.unwrap_or_default().split('&').filter(|p| !p.is_empty()) {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            let k = percent_decode(k);
            let v = percent_decode(v);
            match k.as_str() {
                "q[][pacticipant]" => q.selectors.push(Selector {
                    pacticipant: v,
                    ..Default::default()
                }),
                "q[][version]" => {
                    if let Some(s) = q.selectors.last_mut() {
                        s.version = Some(v);
                    }
                }
                "q[][latest]" => {
                    if let Some(s) = q.selectors.last_mut() {
                        s.latest = v == "true";
                    }
                }
                "q[][tag]" => {
                    if let Some(s) = q.selectors.last_mut() {
                        s.tag = Some(v);
                    }
                }
                "q[][branch]" => {
                    if let Some(s) = q.selectors.last_mut() {
                        s.branch = Some(v);
                    }
                }
                "q[][mainBranch]" => {
                    if let Some(s) = q.selectors.last_mut() {
                        s.main_branch = v == "true";
                    }
                }
                "latestby" => q.latestby = Some(v),
                "environment" => q.environment = Some(v),
                "latest" => q.latest = v == "true",
                "tag" => q.tag = Some(v),
                "ignore[][pacticipant]" => q.ignore.push((v, None)),
                "ignore[][version]" => {
                    if let Some(last) = q.ignore.last_mut() {
                        last.1 = Some(v);
                    }
                }
                // The index's can-i-deploy relation uses flat names.
                "pacticipant" => q.selectors.push(Selector {
                    pacticipant: v,
                    ..Default::default()
                }),
                "version" => {
                    if let Some(s) = q.selectors.last_mut() {
                        s.version = Some(v);
                    }
                }
                "to" => {
                    q.latest = true;
                    q.tag = Some(v);
                }
                _ => {}
            }
        }
        q
    }
}

/// One side of a row, with the words the reason strings use for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Side {
    pub pacticipant: Pacticipant,
    pub version: Option<Version>,
    /// "version 1.0.0 of Consumer", "the version of Provider currently in test (1)", ...
    pub description: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// Resolved candidate retained even when Ruby renders a null provider version.
    pub provider_candidate: Option<Version>,
    pub consumer: Side,
    pub provider: Side,
    pub pact: PactPublication,
    pub verification: Option<Verification>,
    pub ignored: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub cause: Cause,
    pub kind: &'static str,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Matrix {
    pub rows: Vec<Row>,
    pub deployable: Option<bool>,
    pub notices: Vec<Notice>,
    pub success: usize,
    pub failed: usize,
    pub unknown: usize,
    pub ignored: usize,
    pub has_ignores: bool,
}

/// How the other side of an integration is chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Environment(Environment),
    Latest {
        tag: Option<String>,
    },
    /// The versions the second selector resolved to, each with its reason wording.
    /// A single pinned version is the one-element case.
    Versions(Vec<(Version, String)>),
    Missing {
        description: String,
        notice: String,
    },
    /// Every version that has a verification (or none).
    Any,
}

fn errors(msg: String) -> Error {
    Error::Validation(json!([msg]))
}

/// Resolve every directed integration between selected participants. A participant can
/// be a consumer, a provider, or both; query ordering does not assign those roles.
/// Load a consistent read snapshot, release it, then evaluate exactly once.
pub async fn decision(db: &Db, q: &MatrixQuery) -> Result<DecisionEvaluation> {
    Ok(evaluation::evaluate(resolve(db, q).await?))
}

/// Compatibility service entry point. All decisions use the same evaluator.
pub async fn matrix(db: &Db, q: &MatrixQuery) -> Result<Matrix> {
    Ok(decision(db, q).await?.into_matrix())
}

/// Materialize all decision evidence and rendering decorations before leaving the snapshot.
pub async fn resolve(db: &Db, q: &MatrixQuery) -> Result<ResolvedMatrix> {
    let mut context = provenance::EvaluationContext {
        id: uuid::Uuid::new_v4().to_string(),
        started_at: chrono::Utc::now().to_rfc3339(),
        evaluated_at: String::new(),
    };
    let snapshot = db.begin_read().await?;
    let mut resolved = load(&snapshot, q).await?;
    context.evaluated_at = chrono::Utc::now().to_rfc3339();
    resolved.context = context;
    snapshot.commit().await?;
    Ok(resolved)
}

async fn load_version_details(
    db: &Db,
    rows: &[Row],
) -> Result<std::collections::HashMap<i64, VersionDetails>> {
    let mut details = std::collections::HashMap::new();
    let versions: std::collections::BTreeMap<i64, &Version> = rows
        .iter()
        .flat_map(|row| [row.consumer.version.as_ref(), row.provider.version.as_ref()])
        .flatten()
        .map(|v| (v.id, v))
        .collect();
    for v in versions.values() {
        let branches = db.branch_versions_for_version(v.id).await?;
        let environments = db.environments_of_version(v.id).await?;
        let mut tags = vec![];
        for tag in db.tags_for_version(v.id).await? {
            let latest = db.is_latest_with_tag(v, &tag.name).await?;
            tags.push((tag, latest));
        }
        details.insert(
            v.id,
            VersionDetails {
                branches,
                environments,
                tags,
            },
        );
    }
    Ok(details)
}

async fn load(db: &Db, q: &MatrixQuery) -> Result<ResolvedMatrix> {
    let mut notices = vec![];
    for (name, version) in &q.ignore {
        if db.find_pacticipant(name).await?.is_none() {
            notices.push(Notice {
                cause: Cause::IgnoreParticipantMissing(name.clone()),
                kind: "warning",
                text: format!(
                    "WARN: Cannot ignore any version of {name} (no such pacticipant exists)"
                ),
            });
        } else if let Some(number) = version
            && db.find_version(name, number).await?.is_none()
        {
            notices.push(Notice {
                cause: Cause::IgnoreVersionMissing {
                    participant: name.clone(),
                    version: number.clone(),
                },
                kind: "warning",
                text: format!(
                    "WARN: Cannot ignore version {number} of {name} (no such version exists)"
                ),
            });
        }
    }
    // Verified: Ruby reports every specified selector whose version does not exist, in
    // selector order, whether or not an integration pairs it with another selector.
    for s in &q.selectors {
        if s.is_name_only() {
            continue;
        }
        let Some(p) = db.find_pacticipant(&s.pacticipant).await? else {
            continue; // matrix_pair answers 400 for an unknown pacticipant
        };
        if resolve_selector(db, &p, s).await?.0.is_empty() {
            notices.push(Notice {
                cause: Cause::MissingSelector(s.clone()),
                kind: "error",
                text: missing_selector_notice(&p, s),
            });
        }
    }
    let mut rows = vec![];
    let mut provenance = provenance::Provenance {
        environment: match &q.environment {
            Some(name) => db.find_environment_by_name(name).await?,
            None => None,
        },
        ..Default::default()
    };
    if q.selectors.len() < 2 {
        let result = matrix_pair(db, q).await?;
        add_provenance(db, q, &mut provenance, &result, vec![0]).await?;
        notices.extend(result.notices);
        rows.extend(result.rows);
    } else {
        for (i, consumer) in q.selectors.iter().enumerate() {
            for (j, provider) in q.selectors.iter().enumerate() {
                if i == j {
                    continue;
                }
                let pair = MatrixQuery {
                    selectors: vec![consumer.clone(), provider.clone()],
                    ..q.clone()
                };
                let result = matrix_pair(db, &pair).await?;
                add_provenance(db, q, &mut provenance, &result, vec![i, j]).await?;
                notices.extend(result.notices);
                rows.extend(result.rows);
            }
        }
    }
    for (index, requested) in q.selectors.iter().enumerate() {
        let participant = db
            .find_pacticipant(&requested.pacticipant)
            .await?
            .expect("validated participant");
        let versions = resolve_selector(db, &participant, requested).await?.0;
        let missing = notices
            .iter()
            .any(|n| matches!(&n.cause, Cause::MissingSelector(s) if s == requested));
        provenance.selectors.push(provenance::SelectorResolution {
            index,
            requested: requested.clone(),
            versions,
            missing,
        });
    }
    let versions = load_version_details(db, &rows).await?;
    Ok(ResolvedMatrix {
        context: provenance::EvaluationContext::default(),
        provenance,
        query: q.clone(),
        rows,
        notices,
        versions,
    })
}

/// Capture counterpart identity from the selected direction, not the Ruby-null side.
async fn add_provenance(
    db: &Db,
    q: &MatrixQuery,
    out: &mut provenance::Provenance,
    selected: &SelectedRows,
    mut selector_indices: Vec<usize>,
) -> Result<()> {
    use provenance::{
        Candidate, ExcludedEvidence, ExclusionKind, MembershipSource, RowProvenance, merge_indices,
    };
    selector_indices.sort_unstable();
    for (row, excluded) in selected
        .rows
        .iter()
        .map(|r| (r, false))
        .chain(selected.excluded.iter().map(|r| (r, true)))
    {
        // Multi-selector pairs are consumer-directed; single-selector provider queries
        // infer a consumer counterpart. The provider candidate remains on Row for actions.
        let candidate = if q.selectors.len() == 1
            && row.consumer.pacticipant.name != q.selectors[0].pacticipant
        {
            row.consumer.version.as_ref()
        } else {
            row.provider_candidate.as_ref()
        };
        let mut candidate_indices = vec![];
        if let Some(version) = candidate {
            let index = match out
                .candidates
                .iter()
                .position(|c| c.version.id == version.id)
            {
                Some(index) => {
                    merge_indices(
                        &mut out.candidates[index].selector_indices,
                        &selector_indices,
                    );
                    index
                }
                None => {
                    let mut sources = vec![];
                    // Environment is selection context only for a single selector. Explicit
                    // counterpart selectors take precedence, so do not claim membership caused them.
                    if q.selectors.len() == 1
                        && let Some(env) = &out.environment
                    {
                        sources.extend(
                            db.deployments_of_version(version.id)
                                .await?
                                .into_iter()
                                .filter(|v| v.environment_id == env.id && v.currently_deployed)
                                .map(MembershipSource::Deployed),
                        );
                        sources.extend(
                            db.releases_of_version(version.id)
                                .await?
                                .into_iter()
                                .filter(|v| v.environment_id == env.id && v.currently_supported)
                                .map(MembershipSource::Released),
                        );
                    }
                    let index = out.candidates.len();
                    out.candidates.push(Candidate {
                        version: version.clone(),
                        selector_indices: selector_indices.clone(),
                        sources,
                    });
                    index
                }
            };
            candidate_indices.push(index);
        }
        if excluded {
            out.excluded.push(ExcludedEvidence {
                kind: ExclusionKind::UnverifiedCandidate,
                row: row.clone(),
                candidate_indices,
                selector_indices: selector_indices.clone(),
                selected_row: None,
            });
        } else {
            out.rows.push(RowProvenance {
                candidate_indices,
                selector_indices: selector_indices.clone(),
                ignore_indices: vec![],
            });
        }
    }
    Ok(())
}

type VersionDescription = Box<dyn Fn(&Version) -> String + Send + Sync>;

async fn resolve_selector(
    db: &Db,
    subject: &Pacticipant,
    subject_selector: &Selector,
) -> Result<(Vec<Version>, VersionDescription)> {
    let resolved: (Vec<Version>, VersionDescription) = if let Some(number) =
        &subject_selector.version
    {
        let v = db.find_version(&subject.name, number).await?;
        let name = subject.name.clone();
        (
            v.into_iter().collect(),
            Box::new(move |v: &Version| format!("version {} of {}", v.number, name)),
        )
    } else if subject_selector.latest {
        let name = subject.name.clone();
        if subject_selector.main_branch {
            let branch = subject.main_branch.clone();
            let v = match &branch {
                Some(b) => db.latest_version_in_branch(subject.id, b).await?,
                None => None,
            };
            let b = branch.unwrap_or_default();
            (
                v.into_iter().collect(),
                Box::new(move |v: &Version| {
                    format!(
                        "the latest version of {name} from branch {b} ({})",
                        v.number
                    )
                }),
            )
        } else if let Some(b) = subject_selector.branch.clone() {
            let v = db.latest_version_in_branch(subject.id, &b).await?;
            (
                v.into_iter().collect(),
                Box::new(move |v: &Version| {
                    format!(
                        "the latest version of {name} from branch {b} ({})",
                        v.number
                    )
                }),
            )
        } else if let Some(t) = subject_selector.tag.clone() {
            let v = db.latest_version_with_tag(subject.id, &t).await?;
            (
                v.into_iter().collect(),
                Box::new(move |v: &Version| {
                    format!("the latest version of {name} with tag {t} ({})", v.number)
                }),
            )
        } else {
            let v = db.latest_version(subject.id).await?;
            (
                v.into_iter().collect(),
                Box::new(move |v: &Version| format!("the latest version of {name} ({})", v.number)),
            )
        }
    } else if subject_selector.main_branch || subject_selector.branch.is_some() {
        // Verified: a branch selector without `latest` means every version of the branch.
        let name = subject.name.clone();
        let branch = match &subject_selector.branch {
            Some(b) => Some(b.clone()),
            None => subject.main_branch.clone(),
        };
        let versions = match &branch {
            Some(b) => db.versions_in_branch(subject.id, b).await?,
            None => vec![],
        };
        let b = branch.unwrap_or_default();
        let prefix = if versions.len() > 1 {
            "one of the versions"
        } else {
            "the version"
        };
        (
            versions,
            Box::new(move |v: &Version| {
                format!("{prefix} of {name} from branch {b} ({})", v.number)
            }),
        )
    } else if let Some(t) = subject_selector.tag.clone() {
        // Verified: a tag selector without `latest` means every version with the tag.
        let name = subject.name.clone();
        (
            db.versions_with_tag(subject.id, &t).await?,
            Box::new(move |v: &Version| format!("a version of {name} with tag {t} ({})", v.number)),
        )
    } else {
        let name = subject.name.clone();
        (
            db.versions_of(subject.id).await?,
            Box::new(move |_: &Version| format!("any version of {name}")),
        )
    };
    Ok(resolved)
}

/// Ruby's `ResolvedSelector#version_does_not_exist_description`, in its order of checks.
fn missing_selector_notice(subject: &Pacticipant, selector: &Selector) -> String {
    if let Some(tag) = &selector.tag {
        format!("No version with tag {tag} exists for {}", subject.name)
    } else if let Some(branch) = &selector.branch {
        format!("No version of {} from branch {branch} exists", subject.name)
    } else if selector.main_branch {
        format!("No version of {} from the main branch exists", subject.name)
    } else if let Some(number) = &selector.version {
        format!(
            "No pacts or verifications have been published for version {number} of {}",
            subject.name
        )
    } else {
        format!(
            "No pacts or verifications have been published for {}",
            subject.name
        )
    }
}

struct SelectedRows {
    rows: Vec<Row>,
    excluded: Vec<Row>,
    notices: Vec<Notice>,
}

async fn matrix_pair(db: &Db, q: &MatrixQuery) -> Result<SelectedRows> {
    if q.selectors.is_empty() {
        return Err(errors("Please provide 1 or more version selectors.".into()));
    }
    let mut pacticipants = vec![];
    for s in &q.selectors {
        let p = db
            .find_pacticipant(&s.pacticipant)
            .await?
            .ok_or_else(|| errors(format!("Pacticipant {} not found", s.pacticipant)))?;
        pacticipants.push(p);
    }
    let environment = match &q.environment {
        Some(name) => Some(
            db.find_environment_by_name(name)
                .await?
                .ok_or_else(|| errors(format!("Environment with name '{name}' does not exist")))?,
        ),
        None => None,
    };
    let subject_selector = &q.selectors[0];
    let subject = &pacticipants[0];
    let mut notices: Vec<Notice> = vec![];

    let (subject_versions, subject_description) =
        resolve_selector(db, subject, subject_selector).await?;

    // How the other side is chosen.
    let target = if q.selectors.len() > 1 {
        let other = &pacticipants[1];
        let selector = &q.selectors[1];
        if selector.is_name_only() {
            Target::Any
        } else {
            let (versions, describe) = resolve_selector(db, other, selector).await?;
            if versions.is_empty() {
                Target::Missing {
                    description: format!("a requested version of {}", other.name),
                    notice: missing_selector_notice(other, selector),
                }
            } else {
                Target::Versions(
                    versions
                        .into_iter()
                        .map(|v| {
                            let d = describe(&v);
                            (v, d)
                        })
                        .collect(),
                )
            }
        }
    } else if let Some(e) = &environment {
        Target::Environment(e.clone())
    } else if q.latest {
        Target::Latest { tag: q.tag.clone() }
    } else {
        Target::Any
    };
    // Verified: one selector without an environment gets the environment warning first;
    // the integration route (two unpinned selectors) gets none.
    if q.selectors.len() == 1 && environment.is_none() && q.tag.is_none() {
        notices.insert(0, Notice { cause: Cause::EnvironmentUnspecified, kind: "warning", text: "WARN: It is recommended to specify the environment into which you are deploying. Without the environment, this result will not be reliable.".into() });
    }

    let mut rows: Vec<Row> = vec![];
    let mut excluded = vec![];
    if subject_versions.is_empty() {
        // An unconstrained provider may have publications but no provider versions
        // yet. Its opposite direction supplies the unknown row, not a missing pin.
        if q.selectors.len() == 1 || !subject_selector.is_name_only() {
            notices.push(Notice {
                cause: Cause::MissingSelector(subject_selector.clone()),
                kind: "error",
                text: missing_selector_notice(subject, subject_selector),
            });
        }
        return Ok(SelectedRows {
            rows,
            notices,
            excluded,
        });
    }
    for sv in &subject_versions {
        let sdesc = subject_description(sv);
        // As consumer.
        for pact in db.publications_for_consumer_version(sv.id).await? {
            if q.selectors.len() > 1 && pact.provider_name != pacticipants[1].name {
                continue;
            }
            let provider = db
                .find_pacticipant(&pact.provider_name)
                .await?
                .expect("exists");
            let ignored = false;
            match &target {
                Target::Environment(e) => {
                    let ids = db.version_ids_in_environment(provider.id, e.id).await?;
                    if ids.is_empty() {
                        rows.push(Row {
                            provider_candidate: None,
                            consumer: Side { pacticipant: subject.clone(), version: Some(sv.clone()), description: sdesc.clone() },
                            provider: Side { pacticipant: provider.clone(), version: None, description: format!("a version of {} currently in {} (no version is currently recorded as deployed/released in this environment)", provider.name, e.name) },
                            pact: pact.clone(),
                            verification: None,
                            ignored,
                        });
                    }
                    let version_description = if ids.len() > 1 {
                        "one of the versions"
                    } else {
                        "the version"
                    };
                    // Verified: only the versions in the environment that verified the pact
                    // are rows; when none did, a single blank row names the newest one.
                    let mut selected = vec![];
                    let mut unverified = vec![];
                    for id in &ids {
                        let pv = db.find_version_by_id(*id).await?.expect("exists");
                        let verification = db
                            .latest_verification_by_provider_version(pact.pact_version_id, pv.id)
                            .await?;
                        if verification.is_some() {
                            selected.push((pv, verification));
                        } else {
                            unverified.push(pv);
                        }
                    }
                    if selected.is_empty() && !ids.is_empty() {
                        let pv = db.find_version_by_id(ids[0]).await?.expect("exists");
                        selected.push((pv, None));
                    }
                    for pv in unverified {
                        if selected.iter().any(|(v, _)| v.id == pv.id) {
                            continue;
                        }
                        excluded.push(Row {
                            provider_candidate: Some(pv.clone()),
                            consumer: Side {
                                pacticipant: subject.clone(),
                                version: Some(sv.clone()),
                                description: sdesc.clone(),
                            },
                            provider: Side {
                                pacticipant: provider.clone(),
                                version: None,
                                description: format!(
                                    "{version_description} of {} currently in {} ({})",
                                    provider.name, e.name, pv.number
                                ),
                            },
                            pact: pact.clone(),
                            verification: None,
                            ignored,
                        });
                    }
                    for (pv, verification) in selected {
                        rows.push(Row {
                            provider_candidate: Some(pv.clone()),
                            consumer: Side {
                                pacticipant: subject.clone(),
                                version: Some(sv.clone()),
                                description: sdesc.clone(),
                            },
                            provider: Side {
                                pacticipant: provider.clone(),
                                version: verification.as_ref().map(|_| pv.clone()),
                                description: format!(
                                    "{version_description} of {} currently in {} ({})",
                                    provider.name, e.name, pv.number
                                ),
                            },
                            pact: pact.clone(),
                            verification,
                            ignored,
                        });
                    }
                }
                Target::Latest { tag } => {
                    let pv = match tag {
                        Some(t) => db.latest_version_with_tag(provider.id, t).await?,
                        None => db.latest_version(provider.id).await?,
                    };
                    match pv {
                        Some(pv) => {
                                let verification = db.latest_verification_by_provider_version(pact.pact_version_id, pv.id).await?;
                            let description = match tag {
                                Some(t) => format!("the latest version of {} with tag {t} ({})", provider.name, pv.number),
                                None => format!("the latest version of {} ({})", provider.name, pv.number),
                            };
                            rows.push(Row {
                            provider_candidate: Some(pv.clone()),
                                consumer: Side { pacticipant: subject.clone(), version: Some(sv.clone()), description: sdesc.clone() },
                                provider: Side { pacticipant: provider.clone(), version: verification.as_ref().map(|_| pv.clone()), description },
                                pact: pact.clone(),
                                verification,
                                ignored,
                            });
                        }
                        None => rows.push(Row {
                            provider_candidate: None,
                            consumer: Side { pacticipant: subject.clone(), version: Some(sv.clone()), description: sdesc.clone() },
                            provider: Side { pacticipant: provider.clone(), version: None, description: match tag {
                                Some(t) => format!("the latest version of {} with tag {t} (no such version exists)", provider.name),
                                None => format!("the latest version of {} (no such version exists)", provider.name),
                            } },
                            pact: pact.clone(),
                            verification: None,
                            ignored,
                        }),
                    }
                }
                Target::Missing {
                    description,
                    notice,
                } => {
                    notices.push(Notice {
                        cause: Cause::MissingSelector(q.selectors[1].clone()),
                        kind: "error",
                        text: notice.clone(),
                    });
                    rows.push(Row {
                        provider_candidate: None,
                        consumer: Side {
                            pacticipant: subject.clone(),
                            version: Some(sv.clone()),
                            description: sdesc.clone(),
                        },
                        provider: Side {
                            pacticipant: provider.clone(),
                            version: None,
                            description: description.clone(),
                        },
                        pact: pact.clone(),
                        verification: None,
                        ignored,
                    });
                }
                Target::Versions(versions) => {
                    // Verified: every selected version with a verification is a row; when
                    // none has one, a single blank row describes the newest selected version.
                    let mut verified = vec![];
                    for (pv, description) in versions {
                        if let Some(vf) = db
                            .latest_verification_by_provider_version(pact.pact_version_id, pv.id)
                            .await?
                        {
                            verified.push((pv, description, vf));
                        }
                    }
                    let blank = (versions.first().expect("non-empty").clone(), None);
                    let selected = if verified.is_empty() {
                        vec![blank]
                    } else {
                        verified
                            .into_iter()
                            .map(|(pv, d, vf)| ((pv.clone(), d.clone()), Some(vf)))
                            .collect()
                    };
                    for (pv, description) in versions {
                        if selected.iter().any(|((v, _), _)| v.id == pv.id) {
                            continue;
                        }
                        excluded.push(Row {
                            provider_candidate: Some(pv.clone()),
                            consumer: Side {
                                pacticipant: subject.clone(),
                                version: Some(sv.clone()),
                                description: sdesc.clone(),
                            },
                            provider: Side {
                                pacticipant: provider.clone(),
                                version: None,
                                description: description.clone(),
                            },
                            pact: pact.clone(),
                            verification: None,
                            ignored,
                        });
                    }
                    for ((pv, description), verification) in selected {
                        rows.push(Row {
                            provider_candidate: Some(pv.clone()),
                            consumer: Side {
                                pacticipant: subject.clone(),
                                version: Some(sv.clone()),
                                description: sdesc.clone(),
                            },
                            provider: Side {
                                pacticipant: provider.clone(),
                                version: verification.as_ref().map(|_| pv.clone()),
                                description,
                            },
                            pact: pact.clone(),
                            verification,
                            ignored,
                        });
                    }
                }
                Target::Any => {
                    let verifications = db
                        .latest_verifications_per_provider_version(pact.pact_version_id)
                        .await?;
                    if verifications.is_empty() {
                        rows.push(Row {
                            provider_candidate: None,
                            consumer: Side {
                                pacticipant: subject.clone(),
                                version: Some(sv.clone()),
                                description: sdesc.clone(),
                            },
                            provider: Side {
                                pacticipant: provider.clone(),
                                version: None,
                                description: format!("any version of {}", provider.name),
                            },
                            pact: pact.clone(),
                            verification: None,
                            ignored,
                        });
                    }
                    // Verified: Ruby describes every row with the provider's one resolved
                    // selector: "any version of P" when the query names the provider, else
                    // the inferred selector, which is its latest version.
                    let any_description = if q.selectors.len() > 1 {
                        format!("any version of {}", provider.name)
                    } else {
                        match db.latest_version(provider.id).await? {
                            Some(latest) => {
                                format!("version {} of {}", latest.number, provider.name)
                            }
                            None => format!("any version of {}", provider.name),
                        }
                    };
                    for vf in verifications {
                        let pv = db
                            .find_version_by_id(vf.provider_version_id)
                            .await?
                            .expect("exists");
                        rows.push(Row {
                            provider_candidate: Some(pv.clone()),
                            consumer: Side {
                                pacticipant: subject.clone(),
                                version: Some(sv.clone()),
                                description: sdesc.clone(),
                            },
                            provider: Side {
                                pacticipant: provider.clone(),
                                version: Some(pv.clone()),
                                description: any_description.clone(),
                            },
                            pact: pact.clone(),
                            verification: Some(vf),
                            ignored,
                        });
                    }
                }
            }
        }
        // As provider.
        if q.selectors.len() == 1 {
            for (consumer_id, consumer_name) in db.consumers_of_provider(subject.id).await? {
                let consumer = db.find_pacticipant(&consumer_name).await?.expect("exists");
                let ignored = false;
                let consumer_versions: Vec<(Version, String)> = match &target {
                    Target::Environment(e) => {
                        let mut out = vec![];
                        for id in db.version_ids_in_environment(consumer_id, e.id).await? {
                            let cv = db.find_version_by_id(id).await?.expect("exists");
                            out.push((
                                cv.clone(),
                                format!(
                                    "the version of {consumer_name} currently in {} ({})",
                                    e.name, cv.number
                                ),
                            ));
                        }
                        out
                    }
                    Target::Latest { tag } => {
                        let cv = match tag {
                            Some(t) => db.latest_version_with_tag(consumer_id, t).await?,
                            None => db.latest_version(consumer_id).await?,
                        };
                        cv.into_iter()
                            .map(|cv| {
                                let d = match tag {
                                    Some(t) => format!(
                                        "the latest version of {consumer_name} with tag {t} ({})",
                                        cv.number
                                    ),
                                    None => format!(
                                        "the latest version of {consumer_name} ({})",
                                        cv.number
                                    ),
                                };
                                (cv, d)
                            })
                            .collect()
                    }
                    Target::Versions(_) | Target::Missing { .. } => vec![],
                    Target::Any => {
                        // Every consumer version with a verification by this provider version.
                        let mut out = vec![];
                        for pact in db.publications_for_consumer(consumer_id).await? {
                            if pact.provider_id != subject.id {
                                continue;
                            }
                            if db
                                .latest_verification_by_provider_version(
                                    pact.pact_version_id,
                                    sv.id,
                                )
                                .await?
                                .is_some()
                            {
                                let cv = db
                                    .find_version_by_id(pact.consumer_version_id)
                                    .await?
                                    .expect("exists");
                                out.push((cv.clone(), format!("any version of {consumer_name}")));
                            }
                        }
                        out
                    }
                };
                for (cv, cdesc) in consumer_versions {
                    let Some(pact) = db.find_publication(cv.id, subject.id).await? else {
                        continue;
                    };
                    let verification = db
                        .latest_verification_by_provider_version(pact.pact_version_id, sv.id)
                        .await?;
                    rows.push(Row {
                        provider_candidate: Some(sv.clone()),
                        consumer: Side {
                            pacticipant: consumer.clone(),
                            version: Some(cv.clone()),
                            description: cdesc,
                        },
                        provider: Side {
                            pacticipant: subject.clone(),
                            version: Some(sv.clone()),
                            description: sdesc.clone(),
                        },
                        pact,
                        verification,
                        ignored,
                    });
                }
            }
        }
    }
    Ok(SelectedRows {
        rows,
        notices,
        excluded,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latestby_ties_use_verification_recency_not_provider_creation() {
        let participant = Pacticipant {
            id: 1,
            name: "C".into(),
            display_name: None,
            repository_url: None,
            main_branch: None,
            created_at: "2026-01-01T00:00:00+00:00".into(),
            updated_at: None,
        };
        let consumer = Side {
            pacticipant: participant.clone(),
            version: Some(Version {
                id: 1,
                pacticipant_id: 1,
                pacticipant_name: "C".into(),
                number: "1".into(),
                build_url: None,
                created_at: "2026-01-01T00:00:00+00:00".into(),
                updated_at: None,
            }),
            description: "C1".into(),
        };
        let provider = Side {
            pacticipant: Pacticipant {
                id: 2,
                name: "P".into(),
                ..participant
            },
            version: Some(Version {
                id: 10,
                pacticipant_id: 2,
                pacticipant_name: "P".into(),
                number: "1".into(),
                build_url: None,
                created_at: "2025-01-01T00:00:00+00:00".into(),
                updated_at: None,
            }),
            description: "P1".into(),
        };
        let first = Row {
            provider_candidate: provider.version.clone(),
            consumer,
            provider,
            ignored: false,
            pact: PactPublication {
                id: 1,
                consumer_id: 1,
                consumer_name: "C".into(),
                consumer_version_id: 1,
                consumer_version_number: "1".into(),
                consumer_main_branch: None,
                provider_id: 2,
                provider_name: "P".into(),
                pact_version_id: 1,
                sha: "first".into(),
                content: "{}".into(),
                created_at: "2026-01-01T00:00:00+00:00".into(),
                updated_at: None,
            },
            verification: Some(Verification {
                id: 2,
                number: 2,
                pact_version_id: 1,
                provider_version_id: 10,
                provider_name: "P".into(),
                provider_version_number: "1".into(),
                consumer_version_id: None,
                success: false,
                test_results: None,
                verified_by: None,
                build_url: None,
                created_at: "2026-02-02T00:00:00+00:00".into(),
            }),
        };
        let mut second = first.clone();
        second.consumer.version.as_mut().unwrap().id = 2;
        second.consumer.version.as_mut().unwrap().number = "2".into();
        second.consumer.version.as_mut().unwrap().created_at = "2026-01-02T00:00:00+00:00".into();
        second.pact.id = 2;
        second.pact.consumer_version_id = 2;
        second.pact.consumer_version_number = "2".into();
        second.pact.pact_version_id = 2;
        let verification = second.verification.as_mut().unwrap();
        verification.id = 1;
        verification.pact_version_id = 2;
        verification.success = true;
        verification.created_at = "2026-02-01T00:00:00+00:00".into();
        // Ruby orders the newer verification first, then a stable provider-order
        // sort keeps the last row on the tie, even though its consumer is newer.
        assert_eq!(
            apply_latestby(vec![first, second.clone()], Some("cp")),
            vec![second]
        );
    }

    #[test]
    fn parses_the_cli_query_grammar() {
        let q = MatrixQuery::parse(Some(
            "q%5B%5D%5Bpacticipant%5D=Consumer&q%5B%5D%5Bversion%5D=1.0.0&q%5B%5D%5Bpacticipant%5D=Provider&q%5B%5D%5Blatest%5D=true&q%5B%5D%5Btag%5D=prod&latestby=cvpv&environment=test&ignore%5B%5D%5Bpacticipant%5D=Other&ignore%5B%5D%5Bversion%5D=2",
        ));
        assert_eq!(q.selectors.len(), 2);
        assert_eq!(q.selectors[0].version.as_deref(), Some("1.0.0"));
        assert!(q.selectors[1].latest);
        assert_eq!(q.selectors[1].tag.as_deref(), Some("prod"));
        assert_eq!(q.latestby.as_deref(), Some("cvpv"));
        assert_eq!(q.environment.as_deref(), Some("test"));
        assert_eq!(q.ignore, [("Other".to_owned(), Some("2".to_owned()))]);
        let q = MatrixQuery::parse(Some(
            "q[][pacticipant]=C&q[][latest]=true&q[][mainBranch]=true&latest=true&tag=prod",
        ));
        assert!(q.selectors[0].main_branch && q.selectors[0].latest);
        assert!(q.latest);
        assert_eq!(q.tag.as_deref(), Some("prod"));
        let q = MatrixQuery::parse(Some("pacticipant=C&version=1&to=prod"));
        assert_eq!(q.selectors[0].version.as_deref(), Some("1"));
        assert!(q.latest && q.tag.as_deref() == Some("prod"));
        assert!(MatrixQuery::parse(None).selectors.is_empty());
    }
}
