//! `update`, which replaces the running binary with another release. It resolves no roots.

use clap::Args;

use crate::update::Wanted;
use crate::version::VersionError;

#[derive(Debug, Args)]
pub(crate) struct UpdateArgs {
    /// The release to install, with or without a leading `v`; a release older than this one is
    /// installed too
    #[arg(value_name = "VERSION", default_value = "latest", value_parser = parse_wanted)]
    pub version: Wanted,

    /// Print the running and available versions, and install nothing
    #[arg(long)]
    pub check: bool,
}

/// Parse `latest` or a release version; anything else is a usage error.
fn parse_wanted(raw: &str) -> Result<Wanted, VersionError> {
    Wanted::try_from(raw)
}

#[cfg(test)]
mod tests {
    use crate::cli::Command;
    use crate::cli::testing::{error_kind, parse};
    use crate::update::Wanted;
    use clap::error::ErrorKind;

    #[test]
    fn update_defaults_to_the_latest_release() {
        let Command::Update(args) = parse(&["batfiles", "update"]).command else {
            panic!("expected update");
        };
        assert_eq!(args.version, Wanted::Latest);
        assert!(!args.check);
    }

    #[test]
    fn update_takes_a_version_and_check() {
        let Command::Update(args) = parse(&["batfiles", "update", "v1.2.3", "--check"]).command
        else {
            panic!("expected update");
        };
        assert!(matches!(args.version, Wanted::Release(version) if version.to_string() == "1.2.3"));
        assert!(args.check);
    }

    #[test]
    fn a_malformed_version_is_a_usage_error() {
        assert_eq!(
            error_kind(&["batfiles", "update", "1.2"]),
            ErrorKind::ValueValidation
        );
    }
}
