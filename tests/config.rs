#[macro_use]
mod repos;
mod test_utils;

use git_ai::git::find_repository_in_path;
use repos::test_repo::TestRepo;
use std::fs;

#[test]
fn test_ensure_ai_notes_refspecs_in_remote_push() {
    let test_repo = TestRepo::new();
    let path = test_repo.path().as_path().to_str().unwrap();
    let repo = find_repository_in_path(path).unwrap();

    test_repo
        .git(&[
            "remote",
            "add",
            "origin",
            "https://github.com.not-real/test/test.git",
        ])
        .unwrap();

    repo.ensure_ai_notes_refspecs_in_remote_push("origin")
        .unwrap();

    let config = fs::read_to_string(test_repo.path().join(".git/config")).unwrap();

    // Verify the required refspec was added to the config
    assert!(
        config.contains("push = +refs/notes/ai:refs/notes/ai"),
        "Config should contain the ai notes refspec"
    );
}

#[test]
fn test_ensure_ai_notes_refspecs_fails_when_remote_does_not_exist() {
    let test_repo = TestRepo::new();
    let path = test_repo.path().as_path().to_str().unwrap();
    let repo = find_repository_in_path(path).unwrap();

    // Don't add a remote, so "origin" doesn't exist
    let result = repo.ensure_ai_notes_refspecs_in_remote_push("origin_2_not_here");

    // Should get an error when the remote doesn't exist
    assert!(
        result.is_err(),
        "Should return an error when remote doesn't exist"
    );
}

#[test]
fn test_ensure_ai_notes_refspecs_does_not_duplicate() {
    let test_repo = TestRepo::new();
    let path = test_repo.path().as_path().to_str().unwrap();
    let repo = find_repository_in_path(path).unwrap();

    test_repo
        .git(&[
            "remote",
            "add",
            "origin",
            "https://github.com.not-real/test/test.git",
        ])
        .unwrap();

    // Add the refspec twice
    repo.ensure_ai_notes_refspecs_in_remote_push("origin")
        .unwrap();
    repo.ensure_ai_notes_refspecs_in_remote_push("origin")
        .unwrap();

    let config = fs::read_to_string(test_repo.path().join(".git/config")).unwrap();

    // Count how many times the refspec appears - should only be once
    let count = config
        .matches("push = +refs/notes/ai:refs/notes/ai")
        .count();
    assert_eq!(
        count, 1,
        "Config should contain the ai notes refspec exactly once, found {} times",
        count
    );
}

#[test]
fn test_ensure_ai_notes_refspecs_appends_to_existing() {
    let test_repo = TestRepo::new();
    let path = test_repo.path().as_path().to_str().unwrap();
    let repo = find_repository_in_path(path).unwrap();

    test_repo
        .git(&[
            "remote",
            "add",
            "origin",
            "https://github.com.not-real/test/test.git",
        ])
        .unwrap();

    // Add a different push refspec first
    test_repo
        .git(&[
            "config",
            "--add",
            "remote.origin.push",
            "refs/heads/main:refs/heads/main",
        ])
        .unwrap();

    repo.ensure_ai_notes_refspecs_in_remote_push("origin")
        .unwrap();

    let config = fs::read_to_string(test_repo.path().join(".git/config")).unwrap();

    // Verify both refspecs are present
    assert!(
        config.contains("push = refs/heads/main:refs/heads/main"),
        "Config should still contain the original refspec"
    );
    assert!(
        config.contains("push = +refs/notes/ai:refs/notes/ai"),
        "Config should contain the ai notes refspec"
    );
}
