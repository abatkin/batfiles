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
    LocationInputs, MachineRoots, Roots, RunRoots, detect_os_home, discover_working_repository,
    resolve_machine_roots, resolve_roots, resolve_state_dir,
};
use crate::output::Reporter;
use crate::run::Invocation;
use crate::run::context::DestinationOptions;
use crate::run::lock::RunLock;

/// What every command runs with: the global options, the captured environment,
/// and the reporter. Commands resolve roots from it; it resolves none itself. A
/// command that runs the pipeline builds an [`Invocation`] from it.
pub(crate) struct Session<'a> {
    pub global: &'a GlobalOptions,
    pub env: &'a Environment,
    pub reporter: &'a Reporter,
}

impl Session<'_> {
    /// Resolve config and cache roots without repository discovery, and report them at `-v`.
    pub fn machine_roots(&self) -> Result<MachineRoots, Error> {
        let machine = resolve_machine_roots(&self.location_inputs(), self.env, detect_os_home)?;
        self.report_machine_roots(&machine);
        Ok(machine)
    }

    /// [`Self::machine_roots`], holding the run lock under their cache directory.
    pub fn locked_machine_roots(&self) -> Result<(MachineRoots, RunLock), Error> {
        let machine = self.machine_roots()?;
        let lock = RunLock::acquire(&machine.cache_dir)?;
        Ok((machine, lock))
    }

    /// Resolve every root, for a command that reads the leaf repository.
    pub fn repository_roots(&self) -> Result<Roots, Error> {
        self.roots(discover_working_repository)
    }

    /// [`Self::repository_roots`], holding the run lock under their cache directory.
    pub fn locked_repository_roots(&self) -> Result<(Roots, RunLock), Error> {
        let roots = self.repository_roots()?;
        let lock = RunLock::acquire(&roots.machine.cache_dir)?;
        Ok((roots, lock))
    }

    /// [`Self::repository_roots`] and the state root, for `sync` and the apply commands, holding
    /// the run lock under their cache directory.
    pub fn locked_run_roots(&self) -> Result<(RunRoots, RunLock), Error> {
        let roots = self.run_roots(self.repository_roots()?)?;
        let lock = RunLock::acquire(&roots.repository.machine.cache_dir)?;
        Ok((roots, lock))
    }

    /// Resolve roots for `clone`, without searching for an existing repository.
    pub fn new_run_roots(&self) -> Result<RunRoots, Error> {
        self.run_roots(self.roots(|| Ok(None))?)
    }

    /// The pipeline's inputs for a command that executes actions.
    pub fn invocation<'s>(
        &'s self,
        roots: &'s RunRoots,
        dry_run: bool,
        execution: &'s ExecutionOptions,
    ) -> Invocation<'s> {
        Invocation {
            roots: &roots.repository,
            state_dir: &roots.state_dir,
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
        self.report_machine_roots(&roots.machine);
        Ok(roots)
    }

    /// Add the state root to resolved repository roots, and report it at `-v`.
    fn run_roots(&self, repository: Roots) -> Result<RunRoots, Error> {
        let state_dir = resolve_state_dir(&self.location_inputs(), self.env, detect_os_home)?;
        self.reporter
            .detail(1, &format!("{:<12}{}", "state:", state_dir.display()));
        Ok(RunRoots {
            repository,
            state_dir,
        })
    }

    /// The location options used by root resolution.
    fn location_inputs(&self) -> LocationInputs {
        LocationInputs {
            batfiles_dir: self.global.batfiles_dir.clone(),
            home_dir: self.global.home_dir.clone(),
            config_dir: self.global.config_dir.clone(),
            cache_dir: self.global.cache_dir.clone(),
            state_dir: self.global.state_dir.clone(),
        }
    }

    /// Report the config and cache paths at `-v`.
    fn report_machine_roots(&self, machine: &MachineRoots) {
        for (label, path) in [
            ("config:", &machine.config_dir),
            ("cache:", &machine.cache_dir),
        ] {
            self.reporter
                .detail(1, &format!("{label:<12}{}", path.display()));
        }
    }
}
