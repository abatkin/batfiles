//! `init`: laying the conventional skeleton into the directory it was run in, or only its stubs
//! into a repository already there.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

use super::support::{Tree, batfiles, display, entries, stderr_of};

/// What a fresh `init` leaves behind, sorted as [`entries`] returns it.
const SKELETON: [&str; 6] = [
    ".gitignore",
    "batfiles.toml",
    "bin",
    "files",
    "install.ps1",
    "install.sh",
];

/// An isolated working directory and sibling home for initialization tests.
struct Workspace {
    tree: Tree,
}

impl Workspace {
    fn new() -> Self {
        let tree = Tree::roots();
        fs::create_dir(tree.path("work")).expect("a working directory");
        Self { tree }
    }

    fn dir(&self) -> PathBuf {
        self.tree.path("work")
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir().join(relative)
    }

    /// `init`, run in the working directory.
    fn init(&self) -> Command {
        self.init_in(&self.dir())
    }

    /// Build an `init` command running in `dir`.
    fn init_in(&self, dir: &Path) -> Command {
        let mut command = batfiles();
        command
            .current_dir(dir)
            // Set both platform-specific home variables to keep fixtures independent of the
            // real account.
            .env("HOME", self.tree.path("home"))
            .env("USERPROFILE", self.tree.path("home"))
            // Ignore the developer's Git templates and default branch settings.
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .arg("init");
        command
    }
}

#[test]
fn an_empty_directory_gets_the_whole_skeleton() {
    let workspace = Workspace::new();
    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(entries(&workspace.dir()), SKELETON);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "initialized batfiles repository in {}",
            display(&workspace.dir())
        )),
        "unexpected stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("created batfiles.toml, .gitignore, bin/, files/, install.sh, install.ps1"),
        "the created line is not what was created:\n{stderr}"
    );
}

#[test]
fn a_freshly_initialized_repository_synchronizes() {
    let workspace = Workspace::new();
    workspace.init().arg("--no-git-init").assert().success();

    let assertion = workspace
        .tree
        .batfiles()
        .env("BATFILES_DIR", workspace.dir())
        .arg("sync")
        .assert()
        .success();

    assert!(
        entries(&workspace.tree.path("home")).is_empty(),
        "a starter manifest installed something:\n{}",
        stderr_of(&assertion)
    );
}

#[test]
fn the_exclusion_list_covers_the_generated_remotes_tree() {
    let workspace = Workspace::new();
    workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(
        fs::read_to_string(workspace.path(".gitignore")).expect("a .gitignore"),
        "/remotes/\n"
    );
    assert!(!workspace.path("remotes").exists());
}

#[test]
fn git_initialization_is_skipped_on_request() {
    let workspace = Workspace::new();
    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert!(!workspace.path(".git").exists());
    let stderr = stderr_of(&assertion);
    assert!(
        !stderr.contains("Git repository"),
        "`--no-git-init` still reported Git:\n{stderr}"
    );
}

#[test]
fn a_repository_is_initialized_by_default() {
    let workspace = Workspace::new();
    let assertion = workspace.init().assert().success();

    assert!(workspace.path(".git").is_dir());
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("initialized a Git repository"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn a_directory_already_inside_a_work_tree_keeps_the_repository_it_is_in() {
    let workspace = Workspace::new();
    workspace.init().assert().success();

    let below = workspace.path("below");
    fs::create_dir(&below).expect("a directory below the repository");
    let assertion = workspace.init_in(&below).assert().success();

    assert!(!below.join(".git").exists());
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("a Git repository already covers this directory"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn an_existing_manifest_refuses_the_command() {
    let workspace = Workspace::new();
    fs::write(workspace.path("batfiles.toml"), "").expect("a manifest");

    let assertion = workspace.init().assert().failure().code(1);

    assert_eq!(entries(&workspace.dir()), ["batfiles.toml"]);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("already a batfiles repository"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn a_skeleton_path_of_the_wrong_kind_refuses_the_command() {
    let workspace = Workspace::new();
    fs::write(workspace.path("files"), "not a directory\n").expect("a file in the way");

    let assertion = workspace.init().assert().failure().code(1);

    assert_eq!(entries(&workspace.dir()), ["files"]);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&display(&workspace.path("files"))),
        "the refusal does not name the path:\n{stderr}"
    );
    assert!(
        stderr.contains("is not a directory"),
        "the refusal does not say what the path should be:\n{stderr}"
    );
}

#[test]
fn the_home_directory_is_refused() {
    let workspace = Workspace::new();
    let home = workspace.tree.path("home");

    let assertion = workspace.init_in(&home).assert().failure().code(1);

    assert!(entries(&home).is_empty(), "the home was initialized anyway");
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("is your home directory"),
        "unexpected stderr:\n{stderr}"
    );
}

#[test]
fn an_existing_path_of_the_right_kind_is_left_alone() {
    let workspace = Workspace::new();
    let bin = workspace.path("bin");
    fs::create_dir(&bin).expect("a bin directory");
    fs::write(bin.join("batgrep"), "#!/bin/sh\n").expect("a script");

    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(entries(&workspace.dir()), SKELETON);
    assert_eq!(entries(&bin), ["batgrep"]);
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("created batfiles.toml, .gitignore, files/, install.sh, install.ps1"),
        "`bin/` was reported as created:\n{stderr}"
    );
}

#[test]
fn an_exclusion_list_that_does_not_cover_remotes_is_reported() {
    let workspace = Workspace::new();
    fs::write(workspace.path(".gitignore"), "*.swp\n").expect("an exclusion list");

    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(
        fs::read_to_string(workspace.path(".gitignore")).expect("a .gitignore"),
        "*.swp\n"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("does not ignore the tool-owned `remotes/` tree"),
        "unexpected stderr:\n{stderr}"
    );
}

/// The official release base, which a test build compiles in.
const OFFICIAL_BASE: &str = "https://github.com/abatkin/batfiles/releases";

#[test]
fn the_stub_is_executable_and_stamped_with_the_release_base() {
    let workspace = Workspace::new();
    workspace.init().arg("--no-git-init").assert().success();

    let stub = fs::read_to_string(workspace.path("install.sh")).expect("the stub");
    let mut lines = stub.lines();
    assert_eq!(lines.next(), Some("#!/bin/sh"));
    assert_eq!(lines.next(), Some("# batfiles-stub 1"));
    assert_eq!(
        lines.next().map(str::to_owned),
        Some(format!(
            "BATFILES_BASE=${{BATFILES_BASE:-'{OFFICIAL_BASE}'}}"
        ))
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = fs::metadata(workspace.path("install.sh"))
            .expect("the stub")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}

#[test]
fn the_powershell_stub_is_stamped_with_the_release_base() {
    let workspace = Workspace::new();
    workspace.init().arg("--no-git-init").assert().success();

    let stub = fs::read_to_string(workspace.path("install.ps1")).expect("the stub");
    let mut lines = stub.lines();
    assert_eq!(lines.next(), Some("#Requires -Version 7.0"));
    assert_eq!(lines.next(), Some("# batfiles-stub 1"));
    assert_eq!(
        lines.next().map(str::to_owned),
        Some(format!(
            "$BatfilesBase = if ($env:BATFILES_BASE) {{ $env:BATFILES_BASE }} else {{ \
             '{OFFICIAL_BASE}' }}"
        ))
    );
}

#[test]
fn batfiles_base_chooses_the_base_the_stub_carries() {
    let workspace = Workspace::new();
    workspace
        .init()
        .arg("--no-git-init")
        .env("BATFILES_BASE", "https://example.com/mine/")
        .assert()
        .success();

    let stub = fs::read_to_string(workspace.path("install.sh")).expect("the stub");
    assert!(
        stub.contains("\nBATFILES_BASE=${BATFILES_BASE:-'https://example.com/mine'}\n"),
        "{stub}"
    );
    let stub = fs::read_to_string(workspace.path("install.ps1")).expect("the stub");
    assert!(
        stub.contains("else { 'https://example.com/mine' }\n"),
        "{stub}"
    );
}

#[test]
fn a_base_the_stub_cannot_quote_is_refused_before_anything_is_created() {
    let workspace = Workspace::new();
    let assertion = workspace
        .init()
        .arg("--no-git-init")
        .env("BATFILES_BASE", "https://example.com/it's")
        .assert()
        .code(1);
    assert!(stderr_of(&assertion).contains("check BATFILES_BASE"));
    assert!(entries(&workspace.dir()).is_empty());
}

#[test]
fn an_install_script_of_the_repositorys_own_is_kept_with_a_warning() {
    let workspace = Workspace::new();
    fs::write(workspace.path("install.sh"), "#!/bin/sh\necho mine\n").expect("a script");

    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(
        fs::read_to_string(workspace.path("install.sh")).expect("the script"),
        "#!/bin/sh\necho mine\n"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains("install.sh is not a batfiles stub"),
        "{stderr}"
    );
    assert!(
        stderr.contains("created batfiles.toml, .gitignore, bin/, files/, install.ps1\n"),
        "{stderr}"
    );
}

#[test]
fn an_existing_stub_is_kept_without_a_warning() {
    let workspace = Workspace::new();
    let stub =
        "#!/bin/sh\n# batfiles-stub 1\nBATFILES_BASE=${BATFILES_BASE:-'https://old.example.com'}\n";
    fs::write(workspace.path("install.sh"), stub).expect("a stub");

    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(
        fs::read_to_string(workspace.path("install.sh")).expect("the stub"),
        stub
    );
    assert!(!stderr_of(&assertion).contains("not a batfiles stub"));
}

#[test]
fn a_powershell_script_of_the_repositorys_own_is_kept_with_a_warning() {
    let workspace = Workspace::new();
    fs::write(workspace.path("install.ps1"), "Write-Output mine\n").expect("a script");

    let assertion = workspace.init().arg("--no-git-init").assert().success();

    assert_eq!(
        fs::read_to_string(workspace.path("install.ps1")).expect("the script"),
        "Write-Output mine\n"
    );
    assert!(stderr_of(&assertion).contains("install.ps1 is not a batfiles stub"));
}

/// A repository from before `init` wrote `install.ps1`: everything but that stub.
fn repository_without_the_powershell_stub(workspace: &Workspace) {
    workspace.init().arg("--no-git-init").assert().success();
    fs::remove_file(workspace.path("install.ps1")).expect("an older repository");
    fs::write(workspace.path("batfiles.toml"), "# mine\n").expect("its own manifest");
}

#[test]
fn stubs_adds_only_the_stubs_a_repository_lacks() {
    let workspace = Workspace::new();
    repository_without_the_powershell_stub(&workspace);
    let shell = fs::read_to_string(workspace.path("install.sh")).expect("the stub");

    let assertion = workspace
        .init()
        .arg("--stubs")
        .env("BATFILES_BASE", "https://example.com/mine")
        .assert()
        .success();

    assert_eq!(
        entries(&workspace.dir()),
        SKELETON,
        "no Git repository is created"
    );
    let stub = fs::read_to_string(workspace.path("install.ps1")).expect("the stub");
    assert!(
        stub.contains("else { 'https://example.com/mine' }\n"),
        "{stub}"
    );
    assert_eq!(
        fs::read_to_string(workspace.path("install.sh")).expect("the stub"),
        shell
    );
    assert_eq!(
        fs::read_to_string(workspace.path("batfiles.toml")).expect("the manifest"),
        "# mine\n"
    );
    let stderr = stderr_of(&assertion);
    assert!(
        stderr.contains(&format!(
            "in {}, created install.ps1",
            display(&workspace.dir())
        )),
        "{stderr}"
    );

    let assertion = workspace.init().arg("--stubs").assert().success();
    assert!(stderr_of(&assertion).contains("already has every stub; nothing was added"));
}

#[test]
fn stubs_keeps_a_script_of_the_repositorys_own_with_a_warning() {
    let workspace = Workspace::new();
    repository_without_the_powershell_stub(&workspace);
    fs::write(workspace.path("install.sh"), "#!/bin/sh\necho mine\n").expect("a script");

    let assertion = workspace.init().arg("--stubs").assert().success();

    assert_eq!(
        fs::read_to_string(workspace.path("install.sh")).expect("the script"),
        "#!/bin/sh\necho mine\n"
    );
    assert!(workspace.path("install.ps1").is_file());
    assert!(stderr_of(&assertion).contains("install.sh is not a batfiles stub"));
}

#[test]
fn stubs_needs_a_repository() {
    let workspace = Workspace::new();
    let assertion = workspace.init().arg("--stubs").assert().code(1);
    assert!(stderr_of(&assertion).contains("`init --stubs` adds the stubs to an existing"));
    assert!(entries(&workspace.dir()).is_empty());

    fs::create_dir(workspace.path("batfiles.toml")).expect("a directory in the way");
    workspace.init().arg("--stubs").assert().code(1);
    assert_eq!(entries(&workspace.dir()), ["batfiles.toml"]);

    workspace
        .init()
        .args(["--stubs", "--no-git-init"])
        .assert()
        .code(2);
}

#[test]
fn stubs_refuses_a_stub_path_of_the_wrong_kind() {
    let workspace = Workspace::new();
    repository_without_the_powershell_stub(&workspace);
    fs::create_dir(workspace.path("install.ps1")).expect("a directory in the way");

    let assertion = workspace.init().arg("--stubs").assert().code(1);
    assert!(stderr_of(&assertion).contains("install.ps1 exists and is not a regular file"));
}

#[test]
fn the_leaf_fixture_carries_the_stubs_init_writes() {
    let workspace = Workspace::new();
    workspace.init().arg("--no-git-init").assert().success();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/leaf");
    for stub in ["install.sh", "install.ps1"] {
        assert_eq!(
            fs::read_to_string(fixture.join(stub)).expect("the fixture's stub"),
            fs::read_to_string(workspace.path(stub)).expect("the written stub"),
            "regenerate tests/fixtures/leaf/{stub} with `batfiles init --stubs`"
        );
    }
}
