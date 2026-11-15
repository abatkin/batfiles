//! Each command's entry point. An entry point takes its parsed arguments and the
//! [`Session`], resolves the roots it needs, and takes the run lock if it writes
//! state. `sync`, `clone`, and the apply commands then hand an [`Invocation`] to
//! [`crate::run`].

pub(crate) mod apply;
pub(crate) mod clone;
pub(crate) mod disabled;
pub(crate) mod init;
pub(crate) mod sync;
pub(crate) mod update;
pub(crate) mod vars;

use std::path::PathBuf;

use crate::cli::{ExecutionOptions, GlobalOptions};
use crate::env::Environment;
use crate::error::Error;
use crate::filesystem::mode::RunMode;
use crate::filesystem::replace::ConflictPolicy;
use crate::location::{
    LocationInputs, Roots, StateRoots, detect_os_home, discover_working_repository, resolve_roots,
    resolve_state_roots,
};
use crate::output::Reporter;
use crate::run::Invocation;
use crate::run::context::DestinationOptions;
use crate::run::lock::RunLock;

/// What every command runs with: the global options, the captured environment,
/// and the reporter. Commands resolve roots from it; it resolves none itself.
pub(crate) struct Session<'a> {
    pub global: &'a GlobalOptions,
    pub env: &'a Environment,
    pub reporter: &'a Reporter,
}

impl Session<'_> {
    /// Resolve config and cache roots without repository discovery, and report them at `-v`.
    pub fn state_roots(&self) -> Result<StateRoots, Error> {
        let state = resolve_state_roots(&self.location_inputs(), self.env, detect_os_home)?;
        self.report_state_roots(&state);
        Ok(state)
    }

    /// [`Self::state_roots`], holding the run lock under their cache directory.
    pub fn locked_state_roots(&self) -> Result<(StateRoots, RunLock), Error> {
        let state = self.state_roots()?;
        let lock = RunLock::acquire(&state.cache_dir)?;
        Ok((state, lock))
    }

    /// Resolve every root, for a command that reads the leaf repository.
    pub fn repository_roots(&self) -> Result<Roots, Error> {
        self.roots(discover_working_repository)
    }

    /// [`Self::repository_roots`], holding the run lock under their cache directory.
    pub fn locked_repository_roots(&self) -> Result<(Roots, RunLock), Error> {
        let roots = self.repository_roots()?;
        let lock = RunLock::acquire(&roots.state.cache_dir)?;
        Ok((roots, lock))
    }

    /// Resolve roots for `clone`, without searching for an existing repository.
    pub fn new_repository_roots(&self) -> Result<Roots, Error> {
        self.roots(|| Ok(None))
    }

    /// The pipeline's inputs for a command that executes actions.
    pub fn invocation<'s>(
        &'s self,
        roots: &'s Roots,
        dry_run: bool,
        execution: &'s ExecutionOptions,
    ) -> Invocation<'s> {
        Invocation {
            roots,
            mode: RunMode::new(dry_run),
            vars: &execution.vars,
            refresh_vars: execution.refresh_vars,
            destination_options: DestinationOptions {
                policy: if execution.no_overwrite {
                    ConflictPolicy::Skip
                } else if execution.interactive {
                    ConflictPolicy::Ask
                } else {
                    ConflictPolicy::Backup
                },
                refresh_content: execution.refresh_content,
            },
            env: self.env,
            reporter: self.reporter,
        }
    }

    /// Resolve all roots using the supplied repository discovery function, and report them at `-v`.
    fn roots(
        &self,
        working_repository: impl FnOnce() -> Result<Option<PathBuf>, Error>,
    ) -> Result<Roots, Error> {
        let roots = resolve_roots(
            &self.location_inputs(),
            self.env,
            working_repository,
            detect_os_home,
        )?;
        self.reporter.detail(
            1,
            &format!("{:<12}{}", "repository:", roots.batfiles_repo.display()),
        );
        self.reporter
            .detail(1, &format!("{:<12}{}", "home:", roots.home.display()));
        self.report_state_roots(&roots.state);
        Ok(roots)
    }

    /// The four location options used by root resolution.
    fn location_inputs(&self) -> LocationInputs {
        LocationInputs {
            batfiles_dir: self.global.batfiles_dir.clone(),
            home_dir: self.global.home_dir.clone(),
            config_dir: self.global.config_dir.clone(),
            cache_dir: self.global.cache_dir.clone(),
        }
    }

    /// Report the config and cache paths at `-v`.
    fn report_state_roots(&self, state: &StateRoots) {
        for (label, path) in [("config:", &state.config_dir), ("cache:", &state.cache_dir)] {
            self.reporter
                .detail(1, &format!("{label:<12}{}", path.display()));
        }
    }
}
