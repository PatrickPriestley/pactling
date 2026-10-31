//! Version, branch and tag rules, verified against Ruby broker 2.121.2 (see the Phase 2
//! capture notes in docs/plans/2026-10-30-pact-broker-implementation.md).
use chrono::{DateTime, Utc};

use crate::db::{
    Db,
    pacticipants::Pacticipant,
    versions::{Branch, BranchVersion, Tag, Version},
};
use crate::error::{Error, Result};
use crate::service::pacticipants::{default_display_name, not_found};

/// The Ruby broker's `main_branch_candidates`: the first branch with one of these names
/// becomes the pacticipant's main branch when it has none.
const MAIN_BRANCH_CANDIDATES: [&str; 3] = ["develop", "main", "master"];

/// The Ruby broker's `use_first_tag_as_branch_time_limit` in seconds: a tag put on a
/// version that has no branch yet, this soon after the version was created, also
/// creates a branch of that name.
const FIRST_TAG_AS_BRANCH_WINDOW_SECS: i64 = 10;

/// Everything a version document needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionDetail {
    pub version: Version,
    pub branch_versions: Vec<BranchVersion>,
    pub tags: Vec<Tag>,
    /// Providers this version has pacts with, by name.
    pub providers: Vec<String>,
}

pub async fn find_or_create_pacticipant(db: &Db, name: &str) -> Result<Pacticipant> {
    if let Some(p) = db.find_pacticipant(name).await? {
        return Ok(p);
    }
    let (p, _) = db
        .upsert_pacticipant(name, None, &default_display_name(name), None, None)
        .await?;
    Ok(p)
}

pub async fn detail(db: &Db, version: Version) -> Result<VersionDetail> {
    Ok(VersionDetail {
        branch_versions: db.branch_versions_for_version(version.id).await?,
        tags: db.tags_for_version(version.id).await?,
        providers: db.providers_for_consumer_version(version.id).await?,
        version,
    })
}

pub async fn find(db: &Db, pacticipant: &str, number: &str) -> Result<VersionDetail> {
    let v = db
        .find_version(pacticipant, number)
        .await?
        .ok_or(Error::NoRoute)?;
    detail(db, v).await
}

/// `PUT /pacticipants/{p}/versions/{v}`: creates the pacticipant and version as needed.
/// An existing version gets `updated_at` and the build URL when given.
pub async fn put(
    db: &Db,
    pacticipant: &str,
    number: &str,
    build_url: Option<Option<&str>>,
) -> Result<(VersionDetail, bool)> {
    let p = find_or_create_pacticipant(db, pacticipant).await?;
    let (version, created) = db.find_or_create_version(p.id, number).await?;
    let version = if created && build_url.is_none() {
        version
    } else {
        db.touch_version(version.id, build_url).await?
    };
    Ok((detail(db, version).await?, created))
}

pub async fn delete(db: &Db, pacticipant: &str, number: &str) -> Result<()> {
    let v = db
        .find_version(pacticipant, number)
        .await?
        .ok_or(Error::NoRoute)?;
    db.delete_version(v.id).await?;
    Ok(())
}

/// Unlike every other missing thing under a pacticipant, a missing pacticipant on the
/// versions and branches lists is the HAL 404 with its name (verified).
pub async fn list(db: &Db, pacticipant: &str) -> Result<(Pacticipant, Vec<VersionDetail>)> {
    let p = db
        .find_pacticipant(pacticipant)
        .await?
        .ok_or_else(|| not_found(pacticipant))?;
    let mut out = Vec::new();
    for v in db.list_versions(p.id).await? {
        out.push(detail(db, v).await?);
    }
    Ok((p, out))
}

pub async fn latest(db: &Db, pacticipant: &str) -> Result<VersionDetail> {
    let p = db
        .find_pacticipant(pacticipant)
        .await?
        .ok_or(Error::NoRoute)?;
    let v = db.latest_version(p.id).await?.ok_or(Error::NoRoute)?;
    detail(db, v).await
}

pub async fn latest_with_tag(db: &Db, pacticipant: &str, tag: &str) -> Result<VersionDetail> {
    let p = db
        .find_pacticipant(pacticipant)
        .await?
        .ok_or(Error::NoRoute)?;
    let v = db
        .latest_version_with_tag(p.id, tag)
        .await?
        .ok_or(Error::NoRoute)?;
    detail(db, v).await
}

pub async fn latest_in_branch(db: &Db, pacticipant: &str, branch: &str) -> Result<VersionDetail> {
    let p = db
        .find_pacticipant(pacticipant)
        .await?
        .ok_or(Error::NoRoute)?;
    let v = db
        .latest_version_in_branch(p.id, branch)
        .await?
        .ok_or(Error::NoRoute)?;
    detail(db, v).await
}

// --- tags -----------------------------------------------------------------------------

/// What a tag document needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagDetail {
    pub tag: Tag,
    pub version: Version,
}

pub async fn find_tag(db: &Db, pacticipant: &str, number: &str, tag: &str) -> Result<TagDetail> {
    let version = db
        .find_version(pacticipant, number)
        .await?
        .ok_or(Error::NoRoute)?;
    let tag = db.find_tag(version.id, tag).await?.ok_or(Error::NoRoute)?;
    Ok(TagDetail { tag, version })
}

/// `PUT .../versions/{v}/tags/{t}`: creates pacticipant, version and tag as needed.
pub async fn put_tag(
    db: &Db,
    pacticipant: &str,
    number: &str,
    tag: &str,
) -> Result<(TagDetail, bool)> {
    put_tag_at(db, pacticipant, number, tag, Utc::now()).await
}

/// `put_tag` with an injectable clock for the first-tag-as-branch window.
pub async fn put_tag_at(
    db: &Db,
    pacticipant: &str,
    number: &str,
    tag: &str,
    now: DateTime<Utc>,
) -> Result<(TagDetail, bool)> {
    let p = find_or_create_pacticipant(db, pacticipant).await?;
    let (version, _) = db.find_or_create_version(p.id, number).await?;
    let (t, created) = db.find_or_create_tag(version.id, tag).await?;
    if created && first_tag_becomes_branch(db, &version, now).await? {
        add_to_branch(db, &p, &version, tag).await?;
    }
    Ok((TagDetail { tag: t, version }, created))
}

async fn first_tag_becomes_branch(db: &Db, version: &Version, now: DateTime<Utc>) -> Result<bool> {
    if !db.branch_versions_for_version(version.id).await?.is_empty() {
        return Ok(false);
    }
    let created = DateTime::parse_from_rfc3339(&version.created_at)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| Error::Internal(format!("bad created_at {:?}: {e}", version.created_at)))?;
    Ok((now - created).num_seconds() < FIRST_TAG_AS_BRANCH_WINDOW_SECS)
}

pub async fn delete_tag(db: &Db, pacticipant: &str, number: &str, tag: &str) -> Result<()> {
    let version = db
        .find_version(pacticipant, number)
        .await?
        .ok_or(Error::NoRoute)?;
    if db.delete_tag(version.id, tag).await? {
        Ok(())
    } else {
        Err(Error::NoRoute)
    }
}

// --- branches -------------------------------------------------------------------------

pub async fn find_branch(db: &Db, pacticipant: &str, branch: &str) -> Result<Branch> {
    let p = db
        .find_pacticipant(pacticipant)
        .await?
        .ok_or(Error::NoRoute)?;
    db.find_branch(p.id, branch).await?.ok_or(Error::NoRoute)
}

pub async fn list_branches(db: &Db, pacticipant: &str) -> Result<Vec<Branch>> {
    let p = db
        .find_pacticipant(pacticipant)
        .await?
        .ok_or_else(|| not_found(pacticipant))?;
    Ok(db.list_branches(p.id).await?)
}

pub async fn delete_branch(db: &Db, pacticipant: &str, branch: &str) -> Result<()> {
    let b = find_branch(db, pacticipant, branch).await?;
    db.delete_branch(b.id).await?;
    Ok(())
}

/// Deletes every branch of the pacticipant except the named ones.
pub async fn delete_branches_except(db: &Db, pacticipant: &str, keep: &[String]) -> Result<()> {
    for b in list_branches(db, pacticipant).await? {
        if !keep.iter().any(|k| k == &b.name) {
            db.delete_branch(b.id).await?;
        }
    }
    Ok(())
}

/// What a branch-version document needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchVersionDetail {
    pub branch: Branch,
    pub version: Version,
    pub created_at: String,
}

pub async fn find_branch_version(
    db: &Db,
    pacticipant: &str,
    branch: &str,
    number: &str,
) -> Result<BranchVersionDetail> {
    let b = find_branch(db, pacticipant, branch).await?;
    let version = db
        .find_version(pacticipant, number)
        .await?
        .ok_or(Error::NoRoute)?;
    let created_at = db
        .find_branch_version(b.id, version.id)
        .await?
        .ok_or(Error::NoRoute)?;
    Ok(BranchVersionDetail {
        branch: b,
        version,
        created_at,
    })
}

/// `PUT .../branches/{b}/versions/{v}`: creates pacticipant, version and branch as needed.
pub async fn put_branch_version(
    db: &Db,
    pacticipant: &str,
    branch: &str,
    number: &str,
) -> Result<(BranchVersionDetail, bool)> {
    let p = find_or_create_pacticipant(db, pacticipant).await?;
    let (version, _) = db.find_or_create_version(p.id, number).await?;
    add_to_branch(db, &p, &version, branch).await
}

async fn add_to_branch(
    db: &Db,
    p: &Pacticipant,
    version: &Version,
    branch: &str,
) -> Result<(BranchVersionDetail, bool)> {
    let (b, _) = db.find_or_create_branch(p.id, branch).await?;
    let (created_at, created) = db.find_or_create_branch_version(b.id, version.id).await?;
    if p.main_branch.is_none() && MAIN_BRANCH_CANDIDATES.contains(&branch) {
        db.set_main_branch_if_unset(p.id, branch).await?;
    }
    // Re-read so `updated_at` is current.
    let b = db.find_branch(p.id, branch).await?.expect("branch exists");
    Ok((
        BranchVersionDetail {
            branch: b,
            version: version.clone(),
            created_at,
        },
        created,
    ))
}

pub async fn delete_branch_version(
    db: &Db,
    pacticipant: &str,
    branch: &str,
    number: &str,
) -> Result<()> {
    let bv = find_branch_version(db, pacticipant, branch, number).await?;
    db.delete_branch_version(bv.branch.id, bv.version.id)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn db() -> Db {
        let db = Db::connect("sqlite::memory:").await.unwrap();
        db.migrate().await.unwrap();
        db
    }

    #[tokio::test]
    async fn a_tag_put_within_ten_seconds_of_creation_becomes_the_branch() {
        let db = db().await;
        let (_, created) = put_tag(&db, "C", "1", "prod").await.unwrap();
        assert!(created);
        let v = db.find_version("C", "1").await.unwrap().unwrap();
        let branches = db.branch_versions_for_version(v.id).await.unwrap();
        assert_eq!(branches.len(), 1);
        assert_eq!(branches[0].branch_name, "prod");
        // A second tag does not add a second branch.
        put_tag(&db, "C", "1", "test").await.unwrap();
        assert_eq!(db.branch_versions_for_version(v.id).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn a_tag_put_later_than_ten_seconds_does_not_become_a_branch() {
        let db = db().await;
        put(&db, "C", "1", None).await.unwrap();
        let v = db.find_version("C", "1").await.unwrap().unwrap();
        let later = DateTime::parse_from_rfc3339(&v.created_at)
            .unwrap()
            .with_timezone(&Utc)
            + chrono::Duration::seconds(FIRST_TAG_AS_BRANCH_WINDOW_SECS);
        put_tag_at(&db, "C", "1", "qa", later).await.unwrap();
        assert!(
            db.branch_versions_for_version(v.id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn a_tag_on_a_version_that_already_has_a_branch_does_not_become_a_branch() {
        let db = db().await;
        put_branch_version(&db, "C", "feat", "1").await.unwrap();
        put_tag(&db, "C", "1", "qa").await.unwrap();
        let v = db.find_version("C", "1").await.unwrap().unwrap();
        let names: Vec<String> = db
            .branch_versions_for_version(v.id)
            .await
            .unwrap()
            .into_iter()
            .map(|b| b.branch_name)
            .collect();
        assert_eq!(names, ["feat"]);
    }

    #[tokio::test]
    async fn re_putting_an_existing_tag_is_not_a_creation() {
        let db = db().await;
        assert!(put_tag(&db, "C", "1", "qa").await.unwrap().1);
        assert!(!put_tag(&db, "C", "1", "qa").await.unwrap().1);
    }

    #[tokio::test]
    async fn main_branch_is_detected_from_candidates_only_when_unset() {
        let db = db().await;
        put_branch_version(&db, "C", "feature", "1").await.unwrap();
        assert_eq!(
            db.find_pacticipant("C").await.unwrap().unwrap().main_branch,
            None
        );
        put_branch_version(&db, "C", "develop", "2").await.unwrap();
        assert_eq!(
            db.find_pacticipant("C")
                .await
                .unwrap()
                .unwrap()
                .main_branch
                .as_deref(),
            Some("develop")
        );
        put_branch_version(&db, "C", "main", "3").await.unwrap();
        assert_eq!(
            db.find_pacticipant("C")
                .await
                .unwrap()
                .unwrap()
                .main_branch
                .as_deref(),
            Some("develop")
        );
    }

    #[tokio::test]
    async fn put_version_only_touches_an_existing_version() {
        let db = db().await;
        let (d, created) = put(&db, "C", "1", None).await.unwrap();
        assert!(created);
        assert_eq!(d.version.updated_at, None);
        let (d, created) = put(&db, "C", "1", Some(Some("https://ci/1")))
            .await
            .unwrap();
        assert!(!created);
        assert!(d.version.updated_at.is_some());
        assert_eq!(d.version.build_url.as_deref(), Some("https://ci/1"));
        let (d, _) = put(&db, "C", "1", None).await.unwrap();
        assert_eq!(
            d.version.build_url.as_deref(),
            Some("https://ci/1"),
            "an absent buildUrl keeps the old one"
        );
    }
}
