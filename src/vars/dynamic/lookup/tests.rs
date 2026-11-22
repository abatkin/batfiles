use super::*;

fn repository() -> GithubRepository {
    GithubRepository::try_from("o/r".to_owned()).expect("valid repository")
}

fn answered(status: u16, location: Option<&str>) -> Result<String, LookupError> {
    tag_of(
        &repository(),
        &Unfollowed {
            status,
            location: location.map(str::to_owned),
        },
    )
}

fn reason(status: u16, location: Option<&str>) -> String {
    answered(status, location).expect_err("no tag").to_string()
}

#[test]
fn the_tag_is_everything_after_the_release_route_boundary_decoded() {
    for location in [
        "https://github.com/o/r/releases/tag/v1.2.3",
        "/o/r/releases/tag/v1.2.3",
        "https://github.com/o/r/releases/tag/v1.2.3?x=1#frag",
    ] {
        assert_eq!(answered(302, Some(location)).expect("a tag"), "v1.2.3");
    }
    assert_eq!(
        answered(302, Some("/o/r/releases/tag/release%2F1.0%20rc")).expect("a tag"),
        "release/1.0 rc"
    );
    assert_eq!(
        answered(307, Some("/o/r/releases/tag/feature/releases/tag/v1")).expect("a tag"),
        "feature/releases/tag/v1"
    );
}

#[test]
fn every_redirect_status_is_followed_for_its_location() {
    for status in [301, 302, 303, 307, 308] {
        assert_eq!(
            answered(status, Some("/o/r/releases/tag/v1")).expect("a tag"),
            "v1"
        );
    }
}

#[test]
fn each_answer_without_a_tag_says_what_happened() {
    assert_eq!(reason(404, None), "GitHub has no public repository `o/r`");
    assert_eq!(
        reason(302, Some("https://github.com/o/r/releases")),
        "`o/r` has no published release"
    );
    assert_eq!(
        reason(301, Some("https://github.com/n/m/releases/latest")),
        "`o/r` has moved to `https://github.com/n/m/releases/latest`; update `github-release`"
    );
    assert_eq!(
        reason(200, None),
        "the release lookup was answered with status 200, not a redirect to a release"
    );
    assert_eq!(
        reason(302, Some("/login")),
        "the release lookup was redirected (302) to `/login`, which is not a release"
    );
    assert_eq!(
        reason(302, None),
        "the release lookup was redirected (302) somewhere that is not a release"
    );
    assert_eq!(
        reason(302, Some("/o/r/releases/tag/")),
        "the latest release's tag is empty"
    );
    assert_eq!(
        reason(302, Some("/o/r/releases/tag/%FF")),
        "the latest release's tag is not valid UTF-8"
    );
}

#[test]
fn server_text_is_quoted_only_when_it_is_printable_ascii() {
    assert_eq!(
        reason(301, Some("/n/m\u{7f}/releases/latest")),
        "`o/r` has moved to another repository; update `github-release`"
    );
    assert_eq!(
        LookupError::Request("caf\u{e9}".to_owned()).to_string(),
        "the release lookup failed"
    );
    assert_eq!(
        LookupError::Request("connection refused".to_owned()).to_string(),
        "the release lookup failed: connection refused"
    );
}
