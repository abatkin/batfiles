use super::*;

/// Return a fixed OS home for fallback tests.
fn os_home() -> Result<PathBuf, Error> {
    Ok(PathBuf::from("/os-home"))
}

/// Return a home-resolution error.
fn unavailable() -> Result<PathBuf, Error> {
    Err(Error::HomeUnavailable)
}

fn resolve(cli: LocationInputs, env: &Environment) -> Roots {
    resolve_roots(&cli, env, || Ok(None), os_home).expect("resolution should succeed")
}

fn empty_env() -> Environment {
    Environment::from_pairs([] as [(&str, &str); 0])
}

#[test]
fn defaults_come_from_the_os_home() {
    let roots = resolve(LocationInputs::default(), &empty_env());
    assert_eq!(roots.home, PathBuf::from("/os-home"));
    assert_eq!(roots.batfiles_repo, PathBuf::from("/os-home/dotfiles"));
    assert_eq!(
        roots.machine.config_dir,
        PathBuf::from("/os-home/.config/batfiles")
    );
    assert_eq!(
        roots.machine.cache_dir,
        PathBuf::from("/os-home/.cache/batfiles")
    );
}

#[test]
fn the_environment_overrides_the_defaults() {
    let env = Environment::from_pairs([
        ("BATFILES_HOME", "/env-home"),
        ("BATFILES_DIR", "/env-repo"),
        ("BATFILES_CONFIG_DIR", "/env-config"),
        ("BATFILES_CACHE_DIR", "/env-cache"),
    ]);
    let roots = resolve(LocationInputs::default(), &env);
    assert_eq!(roots.home, PathBuf::from("/env-home"));
    assert_eq!(roots.batfiles_repo, PathBuf::from("/env-repo"));
    assert_eq!(roots.machine.config_dir, PathBuf::from("/env-config"));
    assert_eq!(roots.machine.cache_dir, PathBuf::from("/env-cache"));
}

#[test]
fn options_outrank_the_environment() {
    let env = Environment::from_pairs([
        ("BATFILES_HOME", "/env-home"),
        ("BATFILES_DIR", "/env-repo"),
    ]);
    let cli = LocationInputs {
        home_dir: Some(PathBuf::from("/opt-home")),
        batfiles_dir: Some(PathBuf::from("/opt-repo")),
        ..LocationInputs::default()
    };
    let roots = resolve(cli, &env);
    assert_eq!(roots.home, PathBuf::from("/opt-home"));
    assert_eq!(roots.batfiles_repo, PathBuf::from("/opt-repo"));
}

#[test]
fn the_leaf_repository_default_follows_the_selected_home() {
    let cli = LocationInputs {
        home_dir: Some(PathBuf::from("/alt-home")),
        ..LocationInputs::default()
    };
    let roots = resolve(cli, &empty_env());
    assert_eq!(roots.batfiles_repo, PathBuf::from("/alt-home/dotfiles"));
}

#[test]
fn a_manifest_in_the_working_directory_selects_that_repository() {
    let roots = resolve_roots(
        &LocationInputs::default(),
        &empty_env(),
        || Ok(Some(PathBuf::from("/working-repo"))),
        os_home,
    )
    .expect("resolution should succeed");

    assert_eq!(roots.batfiles_repo, PathBuf::from("/working-repo"));
}

#[test]
fn the_repository_variable_outranks_a_manifest_in_the_working_directory() {
    let env = Environment::from_pairs([("BATFILES_DIR", "/env-repo")]);

    let roots = resolve_roots(
        &LocationInputs::default(),
        &env,
        || Ok(Some(PathBuf::from("/working-repo"))),
        os_home,
    )
    .expect("resolution should succeed");

    assert_eq!(roots.batfiles_repo, PathBuf::from("/env-repo"));
}

#[test]
fn the_repository_option_outranks_a_manifest_in_the_working_directory() {
    let cli = LocationInputs {
        batfiles_dir: Some(PathBuf::from("/option-repo")),
        ..LocationInputs::default()
    };

    let roots = resolve_roots(
        &cli,
        &empty_env(),
        || Ok(Some(PathBuf::from("/working-repo"))),
        os_home,
    )
    .expect("resolution should succeed");

    assert_eq!(roots.batfiles_repo, PathBuf::from("/option-repo"));
}

#[test]
fn machine_roots_resolve_the_way_the_full_set_resolves_them() {
    let env = Environment::from_pairs([
        ("BATFILES_CONFIG_DIR", "/env-config"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
    ]);
    let cli = LocationInputs {
        config_dir: Some(PathBuf::from("/opt-config")),
        ..LocationInputs::default()
    };

    let state = resolve_machine_roots(&cli, &env, os_home).expect("resolution should succeed");

    assert_eq!(state.config_dir, PathBuf::from("/opt-config"));
    assert_eq!(state.cache_dir, PathBuf::from("/xdg/cache/batfiles"));
}

#[test]
fn machine_roots_fall_back_to_the_os_home() {
    let state = resolve_machine_roots(&LocationInputs::default(), &empty_env(), os_home)
        .expect("resolution should succeed");

    assert_eq!(state.config_dir, PathBuf::from("/os-home/.config/batfiles"));
    assert_eq!(state.cache_dir, PathBuf::from("/os-home/.cache/batfiles"));
}

#[test]
fn covered_machine_roots_never_need_a_home_at_all() {
    let env = Environment::from_pairs([
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
    ]);

    let state = resolve_machine_roots(&LocationInputs::default(), &env, unavailable)
        .expect("covered machine roots should not need the OS home");

    assert_eq!(state.config_dir, PathBuf::from("/xdg/config/batfiles"));
}

#[test]
fn a_state_root_left_uncovered_still_requires_the_os_home() {
    let env = Environment::from_pairs([("XDG_CONFIG_HOME", "/xdg/config")]);
    assert!(matches!(
        resolve_machine_roots(&LocationInputs::default(), &env, unavailable),
        Err(Error::HomeUnavailable)
    ));
}

#[test]
fn selecting_a_home_does_not_move_config_or_cache() {
    let cli = LocationInputs {
        home_dir: Some(PathBuf::from("/alt-home")),
        ..LocationInputs::default()
    };
    let env = Environment::from_pairs([("BATFILES_HOME", "/env-home")]);
    let roots = resolve(cli, &env);
    assert_eq!(
        roots.machine.config_dir,
        PathBuf::from("/os-home/.config/batfiles")
    );
    assert_eq!(
        roots.machine.cache_dir,
        PathBuf::from("/os-home/.cache/batfiles")
    );
}

#[test]
fn xdg_bases_default_config_and_cache() {
    let env = Environment::from_pairs([
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
    ]);
    let roots = resolve(LocationInputs::default(), &env);
    assert_eq!(
        roots.machine.config_dir,
        PathBuf::from("/xdg/config/batfiles")
    );
    assert_eq!(
        roots.machine.cache_dir,
        PathBuf::from("/xdg/cache/batfiles")
    );
}

#[test]
fn an_empty_location_variable_falls_through_to_the_default() {
    let env = Environment::from_pairs([("BATFILES_DIR", "")]);
    let roots = resolve(LocationInputs::default(), &env);
    assert_eq!(roots.batfiles_repo, PathBuf::from("/os-home/dotfiles"));
}

#[test]
fn supplying_every_root_never_resolves_the_os_home() {
    let cli = LocationInputs {
        batfiles_dir: Some(PathBuf::from("/repo")),
        home_dir: Some(PathBuf::from("/home")),
        config_dir: Some(PathBuf::from("/config")),
        cache_dir: Some(PathBuf::from("/cache")),
        state_dir: None,
    };
    let roots = resolve_roots(&cli, &empty_env(), || Ok(None), unavailable)
        .expect("explicit roots should not need the OS home");
    assert_eq!(roots.machine.config_dir, PathBuf::from("/config"));
    assert_eq!(roots.machine.cache_dir, PathBuf::from("/cache"));
}

#[test]
fn xdg_bases_remove_the_need_for_an_os_home() {
    let cli = LocationInputs {
        home_dir: Some(PathBuf::from("/home")),
        ..LocationInputs::default()
    };
    let env = Environment::from_pairs([
        ("XDG_CONFIG_HOME", "/xdg/config"),
        ("XDG_CACHE_HOME", "/xdg/cache"),
    ]);
    let roots = resolve_roots(&cli, &env, || Ok(None), unavailable)
        .expect("XDG bases should remove the need for an OS home");
    assert_eq!(roots.home, PathBuf::from("/home"));
    assert_eq!(roots.batfiles_repo, PathBuf::from("/home/dotfiles"));
    assert_eq!(
        roots.machine.config_dir,
        PathBuf::from("/xdg/config/batfiles")
    );
    assert_eq!(
        roots.machine.cache_dir,
        PathBuf::from("/xdg/cache/batfiles")
    );
}

#[test]
fn a_base_left_uncovered_still_requires_the_os_home() {
    // Only the cache root still needs the unavailable OS home.
    let cli = LocationInputs {
        home_dir: Some(PathBuf::from("/home")),
        batfiles_dir: Some(PathBuf::from("/repo")),
        ..LocationInputs::default()
    };
    let env = Environment::from_pairs([("XDG_CONFIG_HOME", "/xdg/config")]);
    assert!(matches!(
        resolve_roots(&cli, &env, || Ok(None), unavailable),
        Err(Error::HomeUnavailable)
    ));
}

#[test]
fn a_missing_home_requires_the_os_home() {
    assert!(matches!(
        resolve_roots(
            &LocationInputs::default(),
            &empty_env(),
            || Ok(None),
            unavailable
        ),
        Err(Error::HomeUnavailable)
    ));
}

#[test]
fn the_state_root_follows_its_precedence() {
    let state = |cli: &LocationInputs, env: &Environment| {
        resolve_state_dir(cli, env, unavailable).expect("an explicit state root needs no home")
    };
    let both = Environment::from_pairs([
        ("BATFILES_STATE_DIR", "/env-state"),
        ("XDG_STATE_HOME", "/xdg/state"),
    ]);
    let option = LocationInputs {
        state_dir: Some(PathBuf::from("/opt-state")),
        ..LocationInputs::default()
    };
    assert_eq!(state(&option, &both), PathBuf::from("/opt-state"));
    assert_eq!(
        state(&LocationInputs::default(), &both),
        PathBuf::from("/env-state")
    );
    let xdg =
        Environment::from_pairs([("BATFILES_STATE_DIR", ""), ("XDG_STATE_HOME", "/xdg/state")]);
    assert_eq!(
        state(&LocationInputs::default(), &xdg),
        PathBuf::from("/xdg/state/batfiles")
    );
}

#[test]
fn the_state_root_falls_back_to_the_os_home_whatever_home_is_selected() {
    let cli = LocationInputs {
        home_dir: Some(PathBuf::from("/alt-home")),
        ..LocationInputs::default()
    };
    let env = Environment::from_pairs([("BATFILES_HOME", "/env-home")]);
    assert_eq!(
        resolve_state_dir(&cli, &env, os_home).expect("resolution should succeed"),
        PathBuf::from("/os-home/.local/state/batfiles")
    );
    assert!(matches!(
        resolve_state_dir(&cli, &env, unavailable),
        Err(Error::HomeUnavailable)
    ));
}
