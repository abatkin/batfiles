//! `update`: replace the running binary with another release from the [release
//! base](../docs/distribution.md#the-release-base). Resolves none of the four roots.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error as ThisError;

use crate::env::Environment;
use crate::error::Error;
use crate::fetch;
use crate::output::Reporter;
use crate::paths;
use crate::release::{self, ReleaseBase};
use crate::version::{Version, VersionError};

/// The release `update` installs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Wanted {
    /// The release the base's `latest/download/VERSION` names, if it is newer.
    Latest,
    /// Exactly this release, whatever the running version.
    Release(Version),
}

impl TryFrom<&str> for Wanted {
    type Error = VersionError;

    /// `latest`, or a version with or without a leading `v`.
    fn try_from(raw: &str) -> Result<Self, Self::Error> {
        if raw == "latest" {
            return Ok(Self::Latest);
        }
        Version::try_from(raw.strip_prefix('v').unwrap_or(raw)).map(Self::Release)
    }
}

/// Install `wanted` over the running executable, or with `check` only report what is running and
/// what is available.
pub(crate) fn run(
    wanted: &Wanted,
    check: bool,
    env: &Environment,
    reporter: &Reporter,
) -> Result<(), Error> {
    let base = ReleaseBase::from_env(env)?;
    let running =
        Version::try_from(crate::cli::VERSION).expect("a build reports a release version");
    reporter.detail(1, &format!("{:<12}{}", "base:", base.as_str()));

    if check {
        let available = match wanted {
            Wanted::Latest => latest(&base)?,
            Wanted::Release(version) => {
                let url = format!("{}/VERSION", release_dir(&base, version));
                let found = read_version(&url)?;
                if &found != version {
                    return Err(UpdateError::OtherRelease {
                        url,
                        expected: version.clone(),
                        found,
                    }
                    .into());
                }
                found
            }
        };
        reporter.data(&format!("running {running}"));
        reporter.data(&format!("available {available}"));
        return Ok(());
    }

    platform_can_replace()?;
    let asset = release::asset_for(release::TARGET).ok_or(UpdateError::NoAsset {
        target: release::TARGET,
    })?;
    let exe = running_executable()?;
    reporter.detail(1, &format!("{:<12}{}", "replacing:", exe.display()));

    // Creating the staged file first is what refuses a directory this user cannot write, before
    // anything is downloaded.
    let staged = paths::beside(&exe, ".batfiles-update");
    let file = create_staged(&staged, &exe)?;
    let installed = choose(wanted, &base, &running, reporter).and_then(|chosen| match chosen {
        Some(chosen) => {
            install(&base, &chosen, asset, file, &staged, &exe, reporter).map(|()| Some(chosen))
        }
        None => Ok(None),
    });
    // An installed release has already been renamed away.
    discard(&staged, reporter);
    if let Some(chosen) = installed? {
        reporter.info(&format!(
            "installed batfiles {chosen} at {}, replacing batfiles {running}",
            exe.display()
        ));
    }
    Ok(())
}

/// The release to install, or `None` when the latest is not newer than `running`.
fn choose(
    wanted: &Wanted,
    base: &ReleaseBase,
    running: &Version,
    reporter: &Reporter,
) -> Result<Option<Version>, Error> {
    match wanted {
        Wanted::Release(version) => Ok(Some(version.clone())),
        Wanted::Latest => {
            let latest = latest(base)?;
            if &latest > running {
                Ok(Some(latest))
            } else {
                reporter.info(&format!(
                    "batfiles {running} is up to date: the latest release is {latest}"
                ));
                Ok(None)
            }
        }
    }
}

/// Download `asset` of `chosen` into the staged `file`, verify it against the release's
/// `SHA256SUMS`, check that it runs and reports `chosen`, and rename it over `exe`.
fn install(
    base: &ReleaseBase,
    chosen: &Version,
    asset: &str,
    mut file: fs::File,
    staged: &Path,
    exe: &Path,
    reporter: &Reporter,
) -> Result<(), Error> {
    let from = release_dir(base, chosen);
    let sums_url = format!("{from}/SHA256SUMS");
    let sums = String::from_utf8_lossy(&fetch::download_to_memory(&sums_url)?).into_owned();
    let expected = digest_for(&sums, asset).ok_or_else(|| UpdateError::NotInSums {
        url: sums_url.clone(),
        asset: asset.to_owned(),
    })?;

    reporter.info(&format!("downloading {asset} {chosen} from {from}"));
    let url = format!("{from}/{asset}");
    fetch::download(&url, Some(expected), &mut file, staged).map_err(|error| match error {
        Error::DigestMismatch {
            expected, actual, ..
        } => UpdateError::DigestMismatch {
            url: url.clone(),
            sums: sums_url,
            expected,
            actual,
        }
        .into(),
        other => other,
    })?;
    make_executable(&file, staged)?;
    // A file still open for writing cannot be executed.
    drop(file);

    let output = Command::new(staged)
        .arg("version")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|source| UpdateError::DoesNotRun {
            asset: asset.to_owned(),
            source,
        })?;
    let reported = String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_owned();
    let expected = format!("batfiles {chosen}");
    if !output.status.success() || reported != expected {
        return Err(UpdateError::WrongVersion {
            asset: asset.to_owned(),
            expected,
            reported,
        }
        .into());
    }

    fs::rename(staged, exe).map_err(|source| Error::Write {
        path: exe.to_path_buf(),
        source,
    })
}

/// The version `<base>/latest/download/VERSION` names.
fn latest(base: &ReleaseBase) -> Result<Version, Error> {
    let url = format!("{}/latest/download/VERSION", base.as_str());
    read_version(&url).map_err(|source| {
        UpdateError::NoLatest {
            url,
            source: Box::new(source),
        }
        .into()
    })
}

/// The version a release's `VERSION` document holds.
fn read_version(url: &str) -> Result<Version, Error> {
    let body = fetch::download_to_memory(url)?;
    let text = String::from_utf8_lossy(&body);
    let line = text.strip_suffix('\n').unwrap_or(&text);
    Version::try_from(line).map_err(|_| {
        UpdateError::NotAVersion {
            url: url.to_owned(),
            content: line.escape_default().to_string(),
        }
        .into()
    })
}

/// Where one release's assets are.
fn release_dir(base: &ReleaseBase, version: &Version) -> String {
    format!("{}/download/v{version}", base.as_str())
}

/// The digest `SHA256SUMS` lists for `asset`, in `sha256sum` format.
fn digest_for<'a>(sums: &'a str, asset: &str) -> Option<&'a str> {
    sums.lines().find_map(|line| {
        let (digest, name) = line.split_once(char::is_whitespace)?;
        let name = name.trim_start();
        let name = name.strip_prefix('*').unwrap_or(name);
        (name == asset).then_some(digest)
    })
}

/// The file the running process was started from, with symlinks resolved, so a link to batfiles
/// is left alone and what it points at is replaced.
fn running_executable() -> Result<PathBuf, Error> {
    let exe = std::env::current_exe().map_err(|source| UpdateError::NoExecutable { source })?;
    fs::canonicalize(&exe).map_err(|source| Error::Read { path: exe, source })
}

/// Create the private file a release is downloaded into, failing if anything is already there.
fn create_staged(staged: &Path, exe: &Path) -> Result<fs::File, Error> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(staged).map_err(|source| {
        if source.kind() == io::ErrorKind::AlreadyExists {
            Error::StagingPathTaken {
                path: staged.to_path_buf(),
            }
        } else {
            UpdateError::Unwritable {
                exe: exe.to_path_buf(),
                dir: staged.parent().unwrap_or(staged).to_path_buf(),
                source,
            }
            .into()
        }
    })
}

/// Give a complete, verified download the mode an installed binary has.
#[cfg(unix)]
fn make_executable(file: &fs::File, staged: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;

    file.set_permissions(fs::Permissions::from_mode(0o755))
        .map_err(|source| Error::Write {
            path: staged.to_path_buf(),
            source,
        })
}

/// Windows decides executability by name, which the staged file does not need.
#[cfg(not(unix))]
fn make_executable(_file: &fs::File, _staged: &Path) -> Result<(), Error> {
    Ok(())
}

/// Whether this platform can replace the executable it is running from yet.
#[cfg(unix)]
fn platform_can_replace() -> Result<(), UpdateError> {
    Ok(())
}

#[cfg(not(unix))]
fn platform_can_replace() -> Result<(), UpdateError> {
    // CARRY(10.5): rename the running executable aside, then rename the staged one into place.
    Err(UpdateError::NotOnThisPlatform)
}

/// Remove the staged file, saying so if it cannot be.
fn discard(staged: &Path, reporter: &Reporter) {
    match fs::remove_file(staged) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => reporter.warn(&format!(
            "could not remove the incomplete download at {}: {error}",
            staged.display()
        )),
    }
}

/// Why `update` could not install a release. Nothing was replaced.
#[derive(Debug, ThisError)]
pub(crate) enum UpdateError {
    #[error(
        "no release publishes a binary for {target}, the platform this batfiles was built for; \
         build the release you want from source"
    )]
    NoAsset { target: &'static str },

    #[error("could not find the running batfiles executable: {source}")]
    NoExecutable { source: io::Error },

    #[error(
        "cannot replace {}: could not create a file in {}: {source}",
        .exe.display(),
        .dir.display()
    )]
    Unwritable {
        exe: PathBuf,
        dir: PathBuf,
        source: io::Error,
    },

    #[error(
        "could not read the latest release from {url}: {source}; if the base has published \
         only pre-releases, name one, as `batfiles update <version>`"
    )]
    NoLatest { url: String, source: Box<Error> },

    #[error("{url} holds `{content}`, which is not a release version")]
    NotAVersion { url: String, content: String },

    #[error("{url} holds {found}, not {expected}")]
    OtherRelease {
        url: String,
        expected: Version,
        found: Version,
    },

    #[error("{url} lists no {asset}")]
    NotInSums { url: String, asset: String },

    #[error(
        "{url} does not match {sums}; nothing was installed:\n  listed   {expected}\n  \
         received {actual}"
    )]
    DigestMismatch {
        url: String,
        sums: String,
        expected: String,
        actual: String,
    },

    #[error("the downloaded {asset} does not run on this machine: {source}; nothing was installed")]
    DoesNotRun { asset: String, source: io::Error },

    #[error("the downloaded {asset} reports `{reported}`, not `{expected}`; nothing was installed")]
    WrongVersion {
        asset: String,
        expected: String,
        reported: String,
    },

    #[cfg(not(unix))]
    #[error("`update` cannot replace a running batfiles on this platform yet")]
    NotOnThisPlatform,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_requested_release_may_carry_a_leading_v_or_be_latest() {
        let release = |raw: &str| Wanted::Release(Version::try_from(raw).expect("a version"));
        assert_eq!(Wanted::try_from("1.2.3").ok(), Some(release("1.2.3")));
        assert_eq!(
            Wanted::try_from("v1.2.3-rc.1").ok(),
            Some(release("1.2.3-rc.1"))
        );
        assert_eq!(Wanted::try_from("latest").ok(), Some(Wanted::Latest));
        for raw in ["", "v", "vv1.2.3", "V1.2.3", "1.2", "Latest"] {
            assert!(Wanted::try_from(raw).is_err(), "{raw:?}");
        }
    }

    #[test]
    fn a_digest_is_found_by_its_assets_whole_name() {
        let sums = "aaaa  batfiles-x86_64-unknown-linux-musl\n\
                    bbbb *batfiles-aarch64-unknown-linux-musl\n\
                    cccc  batfiles-x86_64-pc-windows-msvc.exe\n";
        assert_eq!(
            digest_for(sums, "batfiles-x86_64-unknown-linux-musl"),
            Some("aaaa")
        );
        assert_eq!(
            digest_for(sums, "batfiles-aarch64-unknown-linux-musl"),
            Some("bbbb")
        );
        assert_eq!(digest_for(sums, "batfiles-x86_64-pc-windows-msvc"), None);
        assert_eq!(digest_for(sums, "batfiles-x86_64"), None);
        assert_eq!(digest_for("", "batfiles-x86_64-apple-darwin"), None);
    }
}
