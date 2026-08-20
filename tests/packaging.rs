//! What a push to main is supposed to produce.
//!
//! The workflow itself cannot be run here, so what is checked is the parts of
//! it that a change could quietly break: the platforms it builds, the rule
//! that decides whether a release is cut, and the names the files go out
//! under. The disk image's name is checked by asking the script for it, which
//! is the same code path the workflow uses.

const WORKFLOW: &str = include_str!("../.github/workflows/release.yml");

fn manifest_version() -> String {
    let manifest = std::fs::read_to_string("Cargo.toml").expect("the manifest should be there");
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|rest| rest.split('"').next())
        .expect("a version line the packaging scripts can read")
        .to_string()
}

/// The version is where the scripts look for it: a `version = "…"` line the
/// workflow's `sed` will find, holding three numbers.
///
/// Both the workflow and `packaging/macos-app.sh` read it with the same
/// expression, and a version written any other way — inherited from a
/// workspace, say — would come out empty and take the version out of every
/// file name without failing anything.
#[test]
fn the_version_is_where_the_packaging_looks_for_it() {
    let version = manifest_version();
    let parts: Vec<_> = version.split('.').collect();
    assert_eq!(
        parts.len(),
        3,
        "the version should be three numbers: {version}"
    );
    assert!(
        parts.iter().all(|p| p.parse::<u32>().is_ok()),
        "and all of them numbers: {version}"
    );
}

/// A push to main builds everything that is shipped.
#[test]
fn a_push_to_main_builds_every_platform_that_is_shipped() {
    assert!(
        WORKFLOW.contains("branches: [main]"),
        "the workflow should run on a push to main"
    );
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
        "x86_64-pc-windows-msvc",
    ] {
        assert!(WORKFLOW.contains(target), "{target} should be built");
    }
    // And the release is made of what those builds produced, rather than of
    // whatever happens to be lying about.
    assert!(WORKFLOW.contains("needs: [version, package]"));
    assert!(WORKFLOW.contains("needs: [version, test]"));
}

/// A release is cut when the version in the manifest is one that has not been
/// released, and not otherwise.
///
/// That is the whole rule: bumping the version in Cargo.toml is what makes a
/// release, and pushing anything else to main builds the four platforms
/// without publishing a second release under a version that already means
/// something else.
#[test]
fn a_release_is_cut_when_the_version_is_a_new_one() {
    assert!(
        WORKFLOW.contains("git ls-remote --exit-code --tags origin \"refs/tags/v$version\""),
        "the version job should ask whether this version is already tagged"
    );
    assert!(
        WORKFLOW.contains("if: needs.version.outputs.release == 'true'"),
        "and the release job should be the only thing that acts on the answer"
    );
    assert!(
        WORKFLOW.contains("tag_name: v${{ needs.version.outputs.version }}"),
        "the tag is the version, made from the commit that changed it"
    );
    // A tag pushed by hand still releases, and is checked against the tree it
    // was put on rather than trusted.
    assert!(WORKFLOW.contains("refs/tags/v*"));
    assert!(
        WORKFLOW.contains("tag v$version against Cargo.toml $in_manifest"),
        "a tag that does not match the manifest should stop the build"
    );
}

/// Every file that goes out carries the version.
#[test]
fn the_files_that_go_out_carry_the_version() {
    assert!(
        WORKFLOW.contains("out=\"zx-rustrum-$VERSION-${{ matrix.name }}\""),
        "the archives should be named for the version and the platform"
    );
    assert!(
        WORKFLOW.contains("VERSION: ${{ needs.version.outputs.version }}"),
        "which is the version the release is of, not one worked out again"
    );
}

/// The two macOS disk images are two files.
///
/// They are built in the same workflow and their artifacts are merged into one
/// directory before the release: images named the same would be one image, and
/// whichever was uploaded second would be the release for both architectures.
#[test]
#[cfg(unix)]
fn the_macos_disk_images_are_named_apart() {
    let name_of = |target: &str| -> String {
        let out = std::process::Command::new("bash")
            .args(["packaging/macos-app.sh", "--dmg-name", target])
            .output()
            .expect("the packaging script should run");
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };

    let arm = name_of("aarch64-apple-darwin");
    let intel = name_of("x86_64-apple-darwin");
    assert_ne!(arm, intel, "one image each: {arm} and {intel}");
    let version = manifest_version();
    for name in [&arm, &intel] {
        assert!(
            name.contains(&version),
            "the image should say which version it is: {name}"
        );
        assert!(name.ends_with(".dmg"), "and be a disk image: {name}");
    }
    assert!(arm.contains("arm64"), "{arm}");
    assert!(intel.contains("x86_64"), "{intel}");
}
