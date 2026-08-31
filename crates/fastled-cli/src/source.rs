//! Cached FastLED source checkout metadata and safe replacement primitives.
//!
//! The compiler owns downloading a checkout, while this module owns the cache
//! contract shared by normal compilation and `fastled source ...` commands.
//! In particular, updates are staged beside the live checkout and the old
//! checkout is restored if publishing the staged checkout fails.

use std::fs;
use std::io;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli::SourceAction;
use crate::path::NormalizedPath;

/// A `master` checkout is considered old after this duration. It remains
/// usable; callers should surface [`master_stale_warning`] rather than fail a
/// build.
pub(crate) const MASTER_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

const RECEIPT_FILE: &str = ".fastled-source.json";

/// Provenance for a downloaded FastLED checkout.
///
/// `fetched_at_unix_secs` intentionally uses a simple UTC epoch timestamp so
/// receipts remain portable and can be inspected without a date-time crate.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SourceReceipt {
    pub(crate) requested_ref: String,
    pub(crate) fetched_at_unix_secs: u64,
    pub(crate) source_url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) resolved_commit: Option<String>,
}

impl SourceReceipt {
    pub(crate) fn new(
        requested_ref: impl Into<String>,
        source_url: impl Into<String>,
        resolved_commit: Option<String>,
        fetched_at: SystemTime,
    ) -> Result<Self> {
        Ok(Self {
            requested_ref: requested_ref.into(),
            fetched_at_unix_secs: fetched_at
                .duration_since(UNIX_EPOCH)
                .context("source receipt fetch time predates the Unix epoch")?
                .as_secs(),
            source_url: source_url.into(),
            resolved_commit,
        })
    }

    pub(crate) fn fetched_at(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.fetched_at_unix_secs)
    }
}

/// The state of one cache entry, suitable for `fastled source status` output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceStatus {
    pub(crate) requested_ref: String,
    pub(crate) repo_dir: NormalizedPath,
    pub(crate) receipt: Option<SourceReceipt>,
    pub(crate) age: Option<Duration>,
    pub(crate) checkout_exists: bool,
}

impl SourceStatus {
    /// A cache is stale only for `master`. Missing metadata is deliberately
    /// stale: older CLI releases did not record when they downloaded master.
    pub(crate) fn is_stale_master(&self) -> bool {
        self.checkout_exists
            && self.requested_ref == "master"
            && (self.receipt.is_none() || self.age.is_none_or(|age| age > MASTER_MAX_AGE))
    }
}

/// Resolve the `~/.fastled/cache` directory.
pub(crate) fn default_cache_base() -> Result<NormalizedPath> {
    let home = dirs::home_dir().context("cannot resolve home directory")?;
    Ok(NormalizedPath::new(home.join(".fastled").join("cache")))
}

/// Resolve the checkout directory for one cache key.
///
/// Ref names are made path-safe before they are used in a deletion or rename
/// operation. Git ref names containing `/` are intentionally not accepted by
/// this management API until a compatible cache-key encoding is introduced.
pub(crate) fn repo_dir(cache_base: &Path, requested_ref: &str) -> Result<NormalizedPath> {
    validate_cache_ref(requested_ref)?;
    Ok(NormalizedPath::new(
        cache_base.join(format!("fastled-{requested_ref}")),
    ))
}

pub(crate) fn receipt_path(repo_dir: &Path) -> NormalizedPath {
    NormalizedPath::new(repo_dir.join(RECEIPT_FILE))
}

pub(crate) fn read_receipt(repo_dir: &Path) -> Result<Option<SourceReceipt>> {
    let path = receipt_path(repo_dir);
    match fs::read_to_string(&path) {
        Ok(text) => serde_json::from_str(&text)
            .with_context(|| format!("parse source receipt {}", path.display()))
            .map(Some),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("read source receipt {}", path.display())),
    }
}

pub(crate) fn source_status(
    cache_base: &Path,
    requested_ref: &str,
    now: SystemTime,
) -> Result<SourceStatus> {
    let repo_dir = repo_dir(cache_base, requested_ref)?;
    let receipt = read_receipt(&repo_dir)?;
    let age = receipt
        .as_ref()
        .and_then(|receipt| now.duration_since(receipt.fetched_at()).ok());
    Ok(SourceStatus {
        requested_ref: requested_ref.to_string(),
        checkout_exists: repo_dir.is_dir(),
        repo_dir,
        receipt,
        age,
    })
}

/// Return the exact user-facing stale-master message, if a warning applies.
///
/// Local checkouts are represented by callers not invoking this cache-only
/// function. Tags, SHA pins, and non-master branches never warn.
pub(crate) fn master_stale_warning(status: &SourceStatus) -> Option<String> {
    if !status.is_stale_master() {
        return None;
    }

    let days = status
        .age
        .map(|age| (age.as_secs() / (24 * 60 * 60)).max(1))
        .unwrap_or(1);
    let unit = if days == 1 { "day old" } else { "days olds" };
    Some(format!(
        "Your copy of FastLED master is {days} {unit}. Update with `fastled source update`."
    ))
}

/// Populate a fresh checkout in a private staging directory and atomically
/// publish it as `fastled-<ref>`.
///
/// `populate` must leave a complete FastLED checkout in the supplied staging
/// directory. The receipt is written only after `populate` succeeds. If any
/// pre-publish operation fails, the live checkout is untouched. If publishing
/// the staged directory fails after the old checkout was moved aside, this
/// function restores the old checkout before returning the failure.
pub(crate) fn update_checkout<F>(
    cache_base: &Path,
    receipt: &SourceReceipt,
    populate: F,
) -> Result<NormalizedPath>
where
    F: FnOnce(&Path) -> Result<()>,
{
    validate_cache_ref(&receipt.requested_ref)?;
    fs::create_dir_all(cache_base)
        .with_context(|| format!("create source cache {}", cache_base.display()))?;

    let live_dir = repo_dir(cache_base, &receipt.requested_ref)?;
    let staging_dir = unique_sibling(cache_base, &receipt.requested_ref, "staging")?;
    fs::create_dir(&staging_dir)
        .with_context(|| format!("create staged source checkout {}", staging_dir.display()))?;

    let staged = (|| -> Result<()> {
        populate(&staging_dir)?;
        if !staging_dir.join("library.json").is_file() {
            bail!(
                "staged FastLED checkout {} is missing library.json",
                staging_dir.display()
            );
        }
        write_receipt(&staging_dir, receipt)
    })();
    if let Err(error) = staged {
        let _ = fs::remove_dir_all(&staging_dir);
        return Err(error);
    }

    let backup_dir = unique_sibling(cache_base, &receipt.requested_ref, "backup")?;
    let had_live_checkout = live_dir.exists();
    if had_live_checkout {
        fs::rename(&live_dir, &backup_dir).with_context(|| {
            format!(
                "stage existing FastLED checkout {} for replacement",
                live_dir.display()
            )
        })?;
    }

    if let Err(error) = fs::rename(&staging_dir, &live_dir) {
        let rollback = if had_live_checkout {
            fs::rename(&backup_dir, &live_dir)
                .context("restore previous FastLED checkout after failed update")
        } else {
            Ok(())
        };
        let _ = fs::remove_dir_all(&staging_dir);
        rollback?;
        return Err(error)
            .with_context(|| format!("publish staged FastLED checkout {}", live_dir.display()));
    }

    if had_live_checkout {
        fs::remove_dir_all(&backup_dir).with_context(|| {
            format!("remove replaced FastLED checkout {}", backup_dir.display())
        })?;
    }
    Ok(live_dir)
}

/// Remove a single source checkout, never the cache root.
pub(crate) fn purge_checkout(cache_base: &Path, requested_ref: &str) -> Result<bool> {
    let repo_dir = repo_dir(cache_base, requested_ref)?;
    match fs::remove_dir_all(&repo_dir) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("purge {}", repo_dir.display())),
    }
}

/// Execute a user-facing `fastled source ...` command.
pub(crate) fn run_source_action(action: SourceAction) -> Result<()> {
    let cache_base = default_cache_base()?;
    match action {
        SourceAction::Status { reference } => {
            let status = source_status(&cache_base, &reference, SystemTime::now())?;
            println!("ref: {}", status.requested_ref);
            println!("path: {}", status.repo_dir.display());
            if !status.checkout_exists {
                println!("state: not cached");
                return Ok(());
            }
            println!("state: cached");
            if let Some(receipt) = &status.receipt {
                println!("source: {}", receipt.source_url);
                println!("fetched at: {}", receipt.fetched_at_unix_secs);
                if let Some(commit) = &receipt.resolved_commit {
                    println!("commit: {commit}");
                }
            } else {
                println!("fetched at: unknown (legacy cache)");
            }
            if let Some(warning) = master_stale_warning(&status) {
                use std::io::IsTerminal;
                if std::io::stderr().is_terminal() {
                    eprintln!("{}", crossterm::style::Stylize::yellow(warning));
                } else {
                    eprintln!("{warning}");
                }
            }
        }
        SourceAction::Update { reference } => {
            let path = crate::install::refresh_fastled_repo(&reference)?;
            println!("Updated FastLED {reference} at {}", path.display());
        }
        SourceAction::Purge { reference } => {
            let removed = purge_checkout(&cache_base, &reference)?;
            crate::install::invalidate_short_fastled_copy(&reference)?;
            if removed {
                println!("Purged FastLED {reference}");
            } else {
                println!("FastLED {reference} is not cached");
            }
        }
    }
    Ok(())
}

pub(crate) fn write_receipt(repo_dir: &Path, receipt: &SourceReceipt) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(receipt).context("serialize source receipt")?;
    fs::write(receipt_path(repo_dir), bytes)
        .with_context(|| format!("write source receipt in {}", repo_dir.display()))
}

fn validate_cache_ref(requested_ref: &str) -> Result<()> {
    if requested_ref.is_empty()
        || requested_ref == "."
        || requested_ref == ".."
        || requested_ref.contains(['/', '\\'])
        || requested_ref.contains("..")
    {
        bail!("unsafe FastLED cache ref {requested_ref:?}");
    }
    Ok(())
}

fn unique_sibling(cache_base: &Path, requested_ref: &str, purpose: &str) -> Result<NormalizedPath> {
    let base = format!(".fastled-{requested_ref}.{purpose}");
    let pid = std::process::id();
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    for counter in 0..1000_u32 {
        let candidate = cache_base.join(format!("{base}-{pid}-{nonce}-{counter}"));
        if !candidate.exists() {
            return Ok(NormalizedPath::new(candidate));
        }
    }
    bail!("could not allocate a unique {purpose} directory for FastLED {requested_ref}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn receipt(ref_name: &str, fetched_at: SystemTime) -> SourceReceipt {
        SourceReceipt::new(
            ref_name,
            "https://github.com/FastLED/FastLED/archive/refs/heads/master.zip",
            Some("deadbeef".to_string()),
            fetched_at,
        )
        .unwrap()
    }

    #[test]
    fn master_warning_uses_exact_command_and_dynamic_days() {
        let cache = TempDir::new().unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(5 * 24 * 60 * 60);
        let old = receipt("master", now - Duration::from_secs(2 * 24 * 60 * 60));
        update_checkout(cache.path(), &old, |staging| {
            fs::write(staging.join("library.json"), "{}").context("write library")
        })
        .unwrap();

        let status = source_status(cache.path(), "master", now).unwrap();
        assert_eq!(
            master_stale_warning(&status).as_deref(),
            Some(
                "Your copy of FastLED master is 2 days olds. Update with `fastled source update`."
            )
        );
    }

    #[test]
    fn master_freshness_uses_a_strict_24_hour_boundary() {
        let cache = TempDir::new().unwrap();
        let now = UNIX_EPOCH + Duration::from_secs(10 * 24 * 60 * 60);
        for (ref_name, age, warns) in [
            ("master", Duration::from_secs(23 * 60 * 60), false),
            ("master", Duration::from_secs(24 * 60 * 60), false),
            ("master", Duration::from_secs(25 * 60 * 60), true),
        ] {
            let item = receipt(ref_name, now - age);
            update_checkout(cache.path(), &item, |staging| {
                fs::write(staging.join("library.json"), "{}").context("write library")
            })
            .unwrap();
            let status = source_status(cache.path(), ref_name, now).unwrap();
            assert_eq!(master_stale_warning(&status).is_some(), warns);
        }
    }

    #[test]
    fn master_without_receipt_is_stale() {
        let cache = TempDir::new().unwrap();
        let repo = repo_dir(cache.path(), "master").unwrap();
        fs::create_dir_all(&repo).unwrap();
        fs::write(repo.join("library.json"), "{}").unwrap();

        let status = source_status(cache.path(), "master", SystemTime::now()).unwrap();
        assert!(status.is_stale_master());
        assert_eq!(
            master_stale_warning(&status).as_deref(),
            Some("Your copy of FastLED master is 1 day old. Update with `fastled source update`.")
        );
    }

    #[test]
    fn tags_shas_and_non_master_branches_never_warn() {
        let cache = TempDir::new().unwrap();
        let now = SystemTime::now();
        for ref_name in ["3.10.0", "abcdef0", "main"] {
            let old = receipt(ref_name, now - Duration::from_secs(7 * 24 * 60 * 60));
            update_checkout(cache.path(), &old, |staging| {
                fs::write(staging.join("library.json"), "{}").context("write library")
            })
            .unwrap();
            let status = source_status(cache.path(), ref_name, now).unwrap();
            assert!(master_stale_warning(&status).is_none(), "{ref_name}");
        }
    }

    #[test]
    fn failed_staged_update_preserves_existing_checkout() {
        let cache = TempDir::new().unwrap();
        let old = receipt("master", SystemTime::now());
        let live = update_checkout(cache.path(), &old, |staging| {
            fs::write(staging.join("library.json"), "{}").context("write library")?;
            fs::write(staging.join("marker"), "old").context("write marker")
        })
        .unwrap();

        let fresh = receipt("master", SystemTime::now());
        assert!(
            update_checkout(cache.path(), &fresh, |_staging| bail!("download failed")).is_err()
        );
        assert_eq!(fs::read_to_string(live.join("marker")).unwrap(), "old");
    }

    #[test]
    fn update_replaces_checkout_and_receipt_together() {
        let cache = TempDir::new().unwrap();
        let old = receipt("master", UNIX_EPOCH + Duration::from_secs(1));
        update_checkout(cache.path(), &old, |staging| {
            fs::write(staging.join("library.json"), "{}").context("write library")?;
            fs::write(staging.join("marker"), "old").context("write marker")
        })
        .unwrap();

        let fresh = receipt("master", UNIX_EPOCH + Duration::from_secs(2));
        let live = update_checkout(cache.path(), &fresh, |staging| {
            fs::write(staging.join("library.json"), "{}").context("write library")?;
            fs::write(staging.join("marker"), "new").context("write marker")
        })
        .unwrap();
        assert_eq!(fs::read_to_string(live.join("marker")).unwrap(), "new");
        assert_eq!(read_receipt(&live).unwrap(), Some(fresh));
    }

    #[test]
    fn purge_is_scoped_to_one_safe_checkout() {
        let cache = TempDir::new().unwrap();
        let master = repo_dir(cache.path(), "master").unwrap();
        let tag = repo_dir(cache.path(), "3.10.0").unwrap();
        fs::create_dir_all(&master).unwrap();
        fs::create_dir_all(&tag).unwrap();

        assert!(purge_checkout(cache.path(), "master").unwrap());
        assert!(!master.exists());
        assert!(tag.exists());
        assert!(repo_dir(cache.path(), "../cache").is_err());
    }
}
