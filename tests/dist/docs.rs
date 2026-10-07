//! `dist/docs.sh`: the documentation's link checks, run with the pinned lychee.

use std::fs;
use std::path::Path;

use tempfile::TempDir;

use super::support::{script, stderr_of, utf8};

/// A built site: an index linking `index`, a nested page with one heading, and a stylesheet.
fn site(index: &str) -> TempDir {
    let dir = TempDir::new().expect("a scratch directory");
    fs::create_dir(dir.path().join("nested")).expect("a nested directory");
    fs::write(
        dir.path().join("nested/page.html"),
        "<h1 id=\"example\">Example</h1>\n",
    )
    .expect("a nested page");
    fs::write(dir.path().join("style.css"), "body {}\n").expect("a stylesheet");
    fs::write(dir.path().join("index.html"), index).expect("an index");
    dir
}

fn html(dir: &Path, site_url: &str) -> assert_cmd::Command {
    script("docs.sh", &["html", utf8(dir), site_url])
}

#[test]
fn a_site_is_checked_as_served_under_its_prefix() {
    let good = site(
        "<a href=\"nested/page.html#example\">Example</a>\
         <link rel=\"stylesheet\" href=\"/project/docs/style.css\">",
    );
    html(good.path(), "/project/docs/").assert().success();

    for (link, target) in [
        ("<a href=\"nested/page.html#missing\">x</a>", "#missing"),
        (
            "<link rel=\"stylesheet\" href=\"missing.css\">",
            "missing.css",
        ),
        ("<a href=\"/docs/index.html\">x</a>", "/docs/index.html"),
    ] {
        let broken = site(link);
        let assertion = html(broken.path(), "/project/docs/").assert().failure();
        let stderr = stderr_of(&assertion);
        assert!(stderr.contains("has broken links"), "{link}: {stderr}");
        assert!(
            stderr.contains(target.trim_start_matches('/')),
            "{link}: {stderr}"
        );
    }
}

#[test]
fn links_to_this_repository_on_github_must_name_files_on_main() {
    let edit = "https://github.com/abatkin/batfiles/edit/main";
    let real = site(&format!("<a href=\"{edit}/docs/README.md\">Edit</a>"));
    html(real.path(), "/").assert().success();
    let missing = site(&format!("<a href=\"{edit}/docs/docs/README.md\">Edit</a>"));
    let assertion = html(missing.path(), "/").assert().failure();
    assert!(stderr_of(&assertion).contains("docs/docs/README.md"));
}

#[test]
fn markdown_sources_check_files_and_headings_but_not_examples() {
    let dir = TempDir::new().expect("a scratch directory");
    let page = dir.path().join("page.md");
    let sources = || script("docs.sh", &["sources", utf8(&page)]);
    let example = "# Page\n\n## `vars list`\n\n```md\n[Example](example-only.md)\n```\n\n\
                   [Here](#vars-list)\n";
    fs::write(&page, example).expect("a page");
    sources().assert().success();

    for (link, target) in [
        ("[Missing](absent.md)", "absent.md"),
        ("[Heading](#missing)", "#missing"),
        (
            "[Source](https://github.com/abatkin/batfiles/blob/main/README.md#missing)",
            "README.md#missing",
        ),
    ] {
        fs::write(&page, format!("{example}\n{link}\n")).expect("a broken page");
        let assertion = sources().assert().failure();
        let stderr = stderr_of(&assertion);
        assert!(stderr.contains("broken links"), "{link}: {stderr}");
        assert!(stderr.contains(target), "{link}: {stderr}");
    }
}
