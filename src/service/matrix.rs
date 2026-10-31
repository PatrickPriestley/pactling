//! The matrix and can-i-deploy: query grammar, row resolution, verdict and the reason
//! strings the CLI prints. Strings and rules follow the Phase 4 capture in the plan.
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
    pub consumer: Side,
    pub provider: Side,
    pub pact: PactPublication,
    pub verification: Option<Verification>,
    pub ignored: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
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
}

/// How the other side of an integration is chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Target {
    Environment(Environment),
    Latest {
        tag: Option<String>,
    },
    Pinned(Version),
    /// Every version that has a verification (or none).
    Any,
}

fn errors(msg: String) -> Error {
    Error::Validation(json!([msg]))
}

pub async fn matrix(db: &Db, q: &MatrixQuery) -> Result<Matrix> {
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

    // The subject versions and how to describe them.
    let (subject_versions, subject_description): (
        Vec<Version>,
        Box<dyn Fn(&Version) -> String + Send + Sync>,
    ) = if let Some(number) = &subject_selector.version {
        let v = db.find_version(&subject.name, number).await?;
        let name = subject.name.clone();
        (
            v.into_iter().collect(),
            Box::new(move |v: &Version| format!("version {} of {}", v.number, name)),
        )
    } else if subject_selector.latest {
        notices.push(Notice { kind: "warning", text: "WARN: It is recommended to specify the version number (rather than the tag or branch) of the pacticipant you wish to deploy to avoid race conditions. Without a version number, this result will not be reliable.".into() });
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
    } else {
        let name = subject.name.clone();
        (
            db.versions_of(subject.id).await?,
            Box::new(move |_: &Version| format!("any version of {name}")),
        )
    };

    // How the other side is chosen.
    let target = if q.selectors.len() > 1 {
        let other = &pacticipants[1];
        match &q.selectors[1].version {
            Some(number) => match db.find_version(&other.name, number).await? {
                Some(v) => Target::Pinned(v),
                None => Target::Any,
            },
            None => Target::Any,
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
    if q.selectors.len() == 1 && environment.is_none() {
        notices.insert(0, Notice { kind: "warning", text: "WARN: It is recommended to specify the environment into which you are deploying. Without the environment, this result will not be reliable.".into() });
    }

    let mut rows: Vec<Row> = vec![];
    if subject_versions.is_empty() {
        let description = match &subject_selector.version {
            Some(n) => format!("version {n} of {}", subject.name),
            None => format!("the latest version of {}", subject.name),
        };
        notices.push(Notice {
            kind: "error",
            text: format!("No pacts or verifications have been published for {description}"),
        });
        return Ok(Matrix {
            rows,
            deployable: Some(false),
            notices,
            success: 0,
            failed: 0,
            unknown: 0,
            ignored: 0,
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
            let ignored = q
                .ignore
                .iter()
                .any(|(p, v)| p == &provider.name && v.as_ref().is_none_or(|_| true));
            match &target {
                Target::Environment(e) => {
                    let ids = db.version_ids_in_environment(provider.id, e.id).await?;
                    if ids.is_empty() {
                        rows.push(Row {
                            consumer: Side { pacticipant: subject.clone(), version: Some(sv.clone()), description: sdesc.clone() },
                            provider: Side { pacticipant: provider.clone(), version: None, description: format!("a version of {} currently in {} (no version is currently recorded as deployed/released in this environment)", provider.name, e.name) },
                            pact: pact.clone(),
                            verification: None,
                            ignored,
                        });
                    }
                    for id in ids {
                        let pv = db.find_version_by_id(id).await?.expect("exists");
                        let verification = db
                            .latest_verification_by_provider_version(pact.pact_version_id, pv.id)
                            .await?;
                        rows.push(Row {
                            consumer: Side {
                                pacticipant: subject.clone(),
                                version: Some(sv.clone()),
                                description: sdesc.clone(),
                            },
                            provider: Side {
                                pacticipant: provider.clone(),
                                version: verification.as_ref().map(|_| pv.clone()),
                                description: format!(
                                    "the version of {} currently in {} ({})",
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
                                consumer: Side { pacticipant: subject.clone(), version: Some(sv.clone()), description: sdesc.clone() },
                                provider: Side { pacticipant: provider.clone(), version: verification.as_ref().map(|_| pv.clone()), description },
                                pact: pact.clone(),
                                verification,
                                ignored,
                            });
                        }
                        None => rows.push(Row {
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
                Target::Pinned(pv) => {
                    let verification = db
                        .latest_verification_by_provider_version(pact.pact_version_id, pv.id)
                        .await?;
                    rows.push(Row {
                        consumer: Side {
                            pacticipant: subject.clone(),
                            version: Some(sv.clone()),
                            description: sdesc.clone(),
                        },
                        provider: Side {
                            pacticipant: provider.clone(),
                            version: Some(pv.clone()),
                            description: format!("version {} of {}", pv.number, provider.name),
                        },
                        pact: pact.clone(),
                        verification,
                        ignored,
                    });
                }
                Target::Any => {
                    let verifications = db
                        .latest_verifications_per_provider_version(pact.pact_version_id)
                        .await?;
                    if verifications.is_empty() {
                        rows.push(Row {
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
                    for vf in verifications {
                        let pv = db
                            .find_version_by_id(vf.provider_version_id)
                            .await?
                            .expect("exists");
                        rows.push(Row {
                            consumer: Side {
                                pacticipant: subject.clone(),
                                version: Some(sv.clone()),
                                description: sdesc.clone(),
                            },
                            provider: Side {
                                pacticipant: provider.clone(),
                                version: Some(pv.clone()),
                                description: format!("version {} of {}", pv.number, provider.name),
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
                let ignored = q.ignore.iter().any(|(p, _)| p == &consumer_name);
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
                    Target::Pinned(_) => vec![],
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
    rows.sort_by(|a, b| {
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
    });

    let (mut success, mut failed, mut unknown, mut ignored) = (0, 0, 0, 0);
    for r in &rows {
        if r.ignored {
            ignored += 1;
            continue;
        }
        match &r.verification {
            Some(v) if v.success => success += 1,
            Some(_) => {
                failed += 1;
                notices.push(Notice {
                    kind: "error",
                    text: format!(
                        "The verification for the pact between {} and {} failed",
                        r.consumer.description, r.provider.description
                    ),
                });
            }
            None => {
                unknown += 1;
                notices.push(Notice {
                    kind: "error",
                    text: format!(
                        "There is no verified pact between {} and {}",
                        r.consumer.description, r.provider.description
                    ),
                });
            }
        }
    }
    let deployable = if failed > 0 {
        Some(false)
    } else if unknown > 0 {
        None
    } else {
        if success > 0 {
            notices.push(Notice {
                kind: "success",
                text: "All required verification results are published and successful".into(),
            });
        } else {
            notices.push(Notice {
                kind: "success",
                text: "There are no missing dependencies".into(),
            });
        }
        Some(true)
    };
    Ok(Matrix {
        rows,
        deployable,
        notices,
        success,
        failed,
        unknown,
        ignored,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
