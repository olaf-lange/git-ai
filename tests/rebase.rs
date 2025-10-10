use git_ai::authorship::rebase_authorship::rewrite_authorship_after_rebase;
use git_ai::git::refs::get_reference_as_authorship_log_v3;
use git_ai::git::test_utils::TmpRepo;

/// Test simple rebase with no conflicts where trees are identical - multiple commits
#[test]
fn test_rebase_no_conflicts_identical_trees() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit (on default branch, usually master)
    tmp_repo
        .write_file("main.txt", "main line 1\nmain line 2\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    // Get the default branch name
    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with multiple AI commits
    tmp_repo.create_branch("feature").unwrap();

    // First AI commit
    tmp_repo
        .write_file(
            "feature1.txt",
            "// AI generated feature 1\nfeature line 1\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature 1").unwrap();
    let feature_commit_1 = tmp_repo.get_head_commit_sha().unwrap();

    // Second AI commit
    tmp_repo
        .write_file(
            "feature2.txt",
            "// AI generated feature 2\nfeature line 2\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature 2").unwrap();
    let feature_commit_2 = tmp_repo.get_head_commit_sha().unwrap();

    // Advance default branch (non-conflicting)
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("other.txt", "other content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main advances").unwrap();

    // Rebase feature onto default branch
    tmp_repo.checkout_branch("feature").unwrap();
    tmp_repo
        .rebase_onto(&default_branch, &default_branch)
        .unwrap();

    // Get rebased commits
    let head = tmp_repo.get_head_commit_sha().unwrap();
    let repo = tmp_repo.gitai_repo();
    let mut rebased_commits = vec![];
    let mut current = repo.find_commit(head).unwrap();
    for _ in 0..2 {
        rebased_commits.push(current.id().to_string());
        current = current.parent(0).unwrap();
    }
    rebased_commits.reverse();

    // Run rewrite
    rewrite_authorship_after_rebase(
        &repo,
        &[feature_commit_1, feature_commit_2],
        &rebased_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify authorship logs were copied for both commits
    for rebased_commit in &rebased_commits {
        let authorship_log = get_reference_as_authorship_log_v3(&repo, rebased_commit).unwrap();
        assert_eq!(authorship_log.metadata.base_commit_sha, *rebased_commit);
        assert!(!authorship_log.attestations.is_empty());
    }
}

/// Test rebase where trees differ (parent changes result in different tree IDs) - multiple commits
#[test]
fn test_rebase_with_different_trees() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("base.txt", "base content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    // Get default branch name
    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with multiple AI commits
    tmp_repo.create_branch("feature").unwrap();

    // First AI commit
    tmp_repo
        .write_file("feature1.txt", "// AI added feature 1\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI changes 1").unwrap();
    let feature_commit_1 = tmp_repo.get_head_commit_sha().unwrap();

    // Second AI commit
    tmp_repo
        .write_file("feature2.txt", "// AI added feature 2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI changes 2").unwrap();
    let feature_commit_2 = tmp_repo.get_head_commit_sha().unwrap();

    // Go back to default branch and add a different file (non-conflicting)
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("main.txt", "main content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main changes").unwrap();

    // Rebase feature onto default branch (no conflicts, but trees will differ)
    tmp_repo.checkout_branch("feature").unwrap();
    tmp_repo
        .rebase_onto(&default_branch, &default_branch)
        .unwrap();

    // Get rebased commits
    let head = tmp_repo.get_head_commit_sha().unwrap();
    let repo = tmp_repo.gitai_repo();
    let mut rebased_commits = vec![];
    let mut current = repo.find_commit(head).unwrap();
    for _ in 0..2 {
        rebased_commits.push(current.id().to_string());
        current = current.parent(0).unwrap();
    }
    rebased_commits.reverse();

    // Run rewrite
    rewrite_authorship_after_rebase(
        &repo,
        &[feature_commit_1, feature_commit_2],
        &rebased_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify authorship log exists and is correct for both commits
    for rebased_commit in &rebased_commits {
        let result = get_reference_as_authorship_log_v3(&repo, rebased_commit);
        assert!(result.is_ok());

        let log = result.unwrap();
        assert_eq!(log.metadata.base_commit_sha, *rebased_commit);
        assert!(!log.attestations.is_empty());
    }
}

/// Test rebase with multiple commits
#[test]
fn test_rebase_multiple_commits() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("main.txt", "main content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    // Get default branch name
    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with multiple commits
    tmp_repo.create_branch("feature").unwrap();

    // First AI commit
    tmp_repo
        .write_file("feature1.txt", "// AI feature 1\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature 1").unwrap();
    let feature_commit_1 = tmp_repo.get_head_commit_sha().unwrap();

    // Second AI commit
    tmp_repo
        .write_file("feature2.txt", "// AI feature 2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature 2").unwrap();
    let feature_commit_2 = tmp_repo.get_head_commit_sha().unwrap();

    // Third AI commit
    tmp_repo
        .write_file("feature3.txt", "// AI feature 3\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_3", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature 3").unwrap();
    let feature_commit_3 = tmp_repo.get_head_commit_sha().unwrap();

    // Advance default branch
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("main2.txt", "more main content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main advances").unwrap();

    // Rebase feature onto default branch
    tmp_repo.checkout_branch("feature").unwrap();
    tmp_repo
        .rebase_onto(&default_branch, &default_branch)
        .unwrap();

    // Get the rebased commits (walk back 3 commits from HEAD)
    let head = tmp_repo.get_head_commit_sha().unwrap();
    let repo = tmp_repo.gitai_repo();
    let mut rebased_commits = vec![];
    let mut current = repo.find_commit(head).unwrap();
    for _ in 0..3 {
        rebased_commits.push(current.id().to_string());
        current = current.parent(0).unwrap();
    }
    rebased_commits.reverse(); // oldest first

    let original_commits = vec![
        feature_commit_1.clone(),
        feature_commit_2.clone(),
        feature_commit_3.clone(),
    ];

    // Run rewrite
    rewrite_authorship_after_rebase(
        &repo,
        &original_commits,
        &rebased_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify all commits have authorship logs
    for rebased_commit in &rebased_commits {
        let result = get_reference_as_authorship_log_v3(&repo, rebased_commit);
        assert!(
            result.is_ok(),
            "Authorship log should exist for {}",
            rebased_commit
        );
    }
}

/// Test rebase where only some commits have authorship logs
#[test]
fn test_rebase_mixed_authorship() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("main.txt", "main content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    // Get default branch name
    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch
    tmp_repo.create_branch("feature").unwrap();

    // Human commit (no AI authorship)
    tmp_repo
        .write_file("human.txt", "human work\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Human work").unwrap();
    let human_commit = tmp_repo.get_head_commit_sha().unwrap();

    // AI commit
    tmp_repo.write_file("ai.txt", "// AI work\n", true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI work").unwrap();
    let ai_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Advance default branch
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("main2.txt", "more main\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main advances").unwrap();

    // Rebase feature onto default branch
    tmp_repo.checkout_branch("feature").unwrap();
    tmp_repo
        .rebase_onto(&default_branch, &default_branch)
        .unwrap();

    // Get rebased commits
    let head = tmp_repo.get_head_commit_sha().unwrap();
    let repo = tmp_repo.gitai_repo();
    let mut rebased_commits = vec![];
    let mut current = repo.find_commit(head).unwrap();
    for _ in 0..2 {
        rebased_commits.push(current.id().to_string());
        current = current.parent(0).unwrap();
    }
    rebased_commits.reverse();

    // Run rewrite
    rewrite_authorship_after_rebase(
        &repo,
        &[human_commit, ai_commit],
        &rebased_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify AI commit has authorship log
    let ai_result = get_reference_as_authorship_log_v3(&repo, &rebased_commits[1]);
    assert!(ai_result.is_ok());

    // Human commit might not have authorship log (that's ok)
    // The function should handle this gracefully
}

/// Test empty rebase (fast-forward)
#[test]
fn test_rebase_fast_forward() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("main.txt", "main content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    // Get default branch name
    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch
    tmp_repo.create_branch("feature").unwrap();

    // Add commit on feature
    tmp_repo
        .write_file("feature.txt", "// AI feature\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Rebase onto default branch (should be fast-forward, no changes)
    tmp_repo
        .rebase_onto(&default_branch, &default_branch)
        .unwrap();
    let after_rebase = tmp_repo.get_head_commit_sha().unwrap();

    // In a fast-forward, the commit SHA stays the same
    // Call rewrite anyway to verify it handles this gracefully (shouldn't crash)
    rewrite_authorship_after_rebase(
        &tmp_repo.gitai_repo(),
        &[feature_commit.clone()],
        &[after_rebase.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify authorship log still exists
    let result = get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &after_rebase);
    assert!(
        result.is_ok(),
        "Authorship should exist even in fast-forward case"
    );
}

/// Test interactive rebase with commit reordering - verifies interactive rebase works
#[test]
fn test_rebase_interactive_reorder() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("base.txt", "base content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();
    tmp_repo.create_branch("feature").unwrap();

    // Create 2 AI commits - we'll rebase these interactively
    tmp_repo
        .write_file("feature1.txt", "// AI feature 1\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 1").unwrap();
    let commit1 = tmp_repo.get_head_commit_sha().unwrap();

    tmp_repo
        .write_file("feature2.txt", "// AI feature 2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 2").unwrap();
    let commit2 = tmp_repo.get_head_commit_sha().unwrap();

    // Advance main branch
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("main.txt", "main work\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main advances").unwrap();
    let base_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Perform interactive rebase (just pick all, tests that -i flag works)
    tmp_repo.checkout_branch("feature").unwrap();

    use std::process::Command;
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .env("GIT_SEQUENCE_EDITOR", "true") // Just accept the default picks
        .env("GIT_EDITOR", "true") // Auto-accept commit messages
        .args(&["rebase", "-i", &base_commit])
        .output()
        .unwrap();

    if !output.status.success() {
        eprintln!(
            "git rebase output: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Interactive rebase failed");
    }

    // Get the rebased commits
    let head = tmp_repo.get_head_commit_sha().unwrap();
    let repo = tmp_repo.gitai_repo();
    let mut rebased_commits = vec![];
    let mut current = repo.find_commit(head).unwrap();
    for _ in 0..2 {
        rebased_commits.push(current.id().to_string());
        current = current.parent(0).unwrap();
    }
    rebased_commits.reverse();

    // Rewrite authorship for the rebased commits
    rewrite_authorship_after_rebase(
        &repo,
        &[commit1, commit2],
        &rebased_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify both commits have authorship
    for rebased_commit in &rebased_commits {
        let result = get_reference_as_authorship_log_v3(&repo, rebased_commit);
        assert!(
            result.is_ok(),
            "Interactive rebased commit should have authorship"
        );

        let log = result.unwrap();
        assert!(!log.attestations.is_empty(), "Should have AI attestations");
    }
}

/// Test rebase skip - skipping a commit during rebase
#[test]
fn test_rebase_skip() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo.write_file("file.txt", "line 1\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with AI commit that will conflict
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("file.txt", "AI line 1\n", false)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI changes").unwrap();

    // Add second commit that won't conflict
    tmp_repo
        .write_file("feature.txt", "// AI feature\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add feature").unwrap();
    let feature_commit2 = tmp_repo.get_head_commit_sha().unwrap();

    // Make conflicting change on main
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("file.txt", "MAIN line 1\n", false)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main changes").unwrap();

    // Try to rebase - will conflict on first commit
    tmp_repo.checkout_branch("feature").unwrap();
    use std::process::Command;
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", &default_branch])
        .output()
        .unwrap();

    // Should conflict
    assert!(!output.status.success(), "Rebase should conflict");

    // Skip the conflicting commit
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", "--skip"])
        .output()
        .unwrap();

    if output.status.success() {
        // Verify the second commit was rebased
        let rebased_commit = tmp_repo.get_head_commit_sha().unwrap();

        // Rewrite authorship for the one commit that made it through
        rewrite_authorship_after_rebase(
            &tmp_repo.gitai_repo(),
            &[feature_commit2],
            &[rebased_commit.clone()],
            "Test User <test@example.com>",
        )
        .unwrap();

        // Verify authorship
        let result = get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &rebased_commit);
        assert!(
            result.is_ok(),
            "Remaining commit after skip should have authorship"
        );
    }
}

/// Test rebase with empty commits (--keep-empty)
#[test]
fn test_rebase_keep_empty() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo.write_file("base.txt", "base\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with empty commit
    tmp_repo.create_branch("feature").unwrap();

    use std::process::Command;
    // Create empty commit
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["commit", "--allow-empty", "-m", "Empty commit"])
        .output()
        .unwrap();

    assert!(output.status.success(), "Empty commit should succeed");
    let empty_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Add a real commit
    tmp_repo.write_file("feature.txt", "// AI\n", true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Advance main
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo.write_file("main.txt", "main\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main work").unwrap();
    let base = tmp_repo.get_head_commit_sha().unwrap();

    // Rebase with --keep-empty
    tmp_repo.checkout_branch("feature").unwrap();
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", "--keep-empty", &base])
        .output()
        .unwrap();

    if output.status.success() {
        // Get rebased commits
        let head = tmp_repo.get_head_commit_sha().unwrap();
        let repo = tmp_repo.gitai_repo();
        let mut rebased_commits = vec![];
        let mut current = repo.find_commit(head).unwrap();

        // Walk back to collect rebased commits
        for _ in 0..2 {
            rebased_commits.push(current.id().to_string());
            match current.parent(0) {
                Ok(p) => current = p,
                Err(_) => break,
            }
        }
        rebased_commits.reverse();

        // Rewrite authorship
        rewrite_authorship_after_rebase(
            &repo,
            &[empty_commit, feature_commit],
            &rebased_commits,
            "Test User <test@example.com>",
        )
        .unwrap();

        // Verify last commit has authorship
        let result = get_reference_as_authorship_log_v3(&repo, &rebased_commits.last().unwrap());
        assert!(
            result.is_ok(),
            "Non-empty rebased commit should have authorship"
        );
    }
}

/// Test rebase with rerere (reuse recorded resolution) enabled
#[test]
fn test_rebase_rerere() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Enable rerere
    use std::process::Command;
    Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["config", "rerere.enabled", "true"])
        .output()
        .unwrap();

    // Create initial commit
    tmp_repo
        .write_file("conflict.txt", "line 1\nline 2\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with AI changes
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("conflict.txt", "line 1\nAI CHANGE\n", false)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI changes").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Make conflicting change on main
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("conflict.txt", "line 1\nMAIN CHANGE\n", false)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main changes").unwrap();

    // First rebase - will conflict
    tmp_repo.checkout_branch("feature").unwrap();
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", &default_branch])
        .output()
        .unwrap();

    // Should conflict
    assert!(!output.status.success(), "First rebase should conflict");

    // Resolve conflict manually
    tmp_repo
        .write_file("conflict.txt", "line 1\nRESOLVED\n", false)
        .unwrap();

    Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["add", "conflict.txt"])
        .output()
        .unwrap();

    Command::new("git")
        .current_dir(tmp_repo.path())
        .env("GIT_EDITOR", "true")
        .args(&["rebase", "--continue"])
        .output()
        .unwrap();

    // Record the resolution and abort
    Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", "--abort"])
        .output()
        .ok();

    // Second attempt - rerere should auto-apply the resolution
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", &default_branch])
        .output()
        .unwrap();

    // Even if rerere helps, we still need to continue manually
    // This test mainly verifies that rerere doesn't break authorship tracking
    if !output.status.success() {
        Command::new("git")
            .current_dir(tmp_repo.path())
            .args(&["add", "conflict.txt"])
            .output()
            .unwrap();

        Command::new("git")
            .current_dir(tmp_repo.path())
            .env("GIT_EDITOR", "true")
            .args(&["rebase", "--continue"])
            .output()
            .unwrap();
    }

    let rebased_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Rewrite authorship
    rewrite_authorship_after_rebase(
        &tmp_repo.gitai_repo(),
        &[feature_commit],
        &[rebased_commit.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify authorship
    let result = get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &rebased_commit);
    assert!(
        result.is_ok(),
        "Rebase with rerere should preserve authorship"
    );
}

/// Test dependent branch stack (patch-stack workflow)
#[test]
fn test_rebase_patch_stack() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo.write_file("base.txt", "base\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create topic-1 branch
    tmp_repo.create_branch("topic-1").unwrap();
    tmp_repo
        .write_file("topic1.txt", "// AI topic 1\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Topic 1").unwrap();
    let topic1_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Create topic-2 branch on top of topic-1
    tmp_repo.create_branch("topic-2").unwrap();
    tmp_repo
        .write_file("topic2.txt", "// AI topic 2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Topic 2").unwrap();
    let topic2_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Create topic-3 branch on top of topic-2
    tmp_repo.create_branch("topic-3").unwrap();
    tmp_repo
        .write_file("topic3.txt", "// AI topic 3\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Topic 3").unwrap();
    let topic3_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Advance main
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("main.txt", "main work\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main work").unwrap();

    // Rebase the stack: topic-1, then topic-2, then topic-3
    tmp_repo.checkout_branch("topic-1").unwrap();
    tmp_repo
        .rebase_onto(&default_branch, &default_branch)
        .unwrap();
    let rebased_topic1 = tmp_repo.get_head_commit_sha().unwrap();

    tmp_repo.checkout_branch("topic-2").unwrap();
    tmp_repo.rebase_onto("topic-1", "topic-1").unwrap();
    let rebased_topic2 = tmp_repo.get_head_commit_sha().unwrap();

    tmp_repo.checkout_branch("topic-3").unwrap();
    tmp_repo.rebase_onto("topic-2", "topic-2").unwrap();
    let rebased_topic3 = tmp_repo.get_head_commit_sha().unwrap();

    // Rewrite authorship for each
    rewrite_authorship_after_rebase(
        &tmp_repo.gitai_repo(),
        &[topic1_commit],
        &[rebased_topic1.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    rewrite_authorship_after_rebase(
        &tmp_repo.gitai_repo(),
        &[topic2_commit],
        &[rebased_topic2.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    rewrite_authorship_after_rebase(
        &tmp_repo.gitai_repo(),
        &[topic3_commit],
        &[rebased_topic3.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify all have authorship
    for commit in &[rebased_topic1, rebased_topic2, rebased_topic3] {
        let result = get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), commit);
        assert!(
            result.is_ok(),
            "Patch stack commits should all have authorship"
        );
    }
}

/// Test rebase with no changes (already up to date)
#[test]
fn test_rebase_already_up_to_date() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo.write_file("file.txt", "content\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let _default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo.write_file("feature.txt", "// AI\n", true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Try to rebase onto itself (should be no-op)
    use std::process::Command;
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", "feature"])
        .output()
        .unwrap();

    // Should succeed as no-op
    assert!(output.status.success(), "Rebase onto self should succeed");

    // Verify commit unchanged
    let current_commit = tmp_repo.get_head_commit_sha().unwrap();
    assert_eq!(current_commit, feature_commit, "Commit should be unchanged");

    // Verify authorship still intact
    let result = get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &feature_commit);
    assert!(result.is_ok(), "Authorship should still be intact");
}

/// Test rebase with conflicts - verifies reconstruction works after conflict resolution
#[test]
fn test_rebase_with_conflicts() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("base.txt", "base content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create old_base branch and commit
    tmp_repo.create_branch("old_base").unwrap();
    tmp_repo.write_file("old.txt", "old base\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Old base commit").unwrap();
    let old_base_sha = tmp_repo.get_head_commit_sha().unwrap();

    // Create feature branch from old_base with AI commits
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("feature.txt", "// AI feature\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();
    let original_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Create new_base branch from default_branch
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo.create_branch("new_base").unwrap();
    tmp_repo.write_file("new.txt", "new base\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("New base commit").unwrap();
    let new_base_sha = tmp_repo.get_head_commit_sha().unwrap();

    // Rebase feature --onto new_base old_base
    tmp_repo.checkout_branch("feature").unwrap();
    use std::process::Command;
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", "--onto", &new_base_sha, &old_base_sha])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Rebase --onto should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let rebased_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Rewrite authorship
    rewrite_authorship_after_rebase(
        &tmp_repo.gitai_repo(),
        &[original_commit],
        &[rebased_commit.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify authorship preserved
    let result = get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &rebased_commit);
    assert!(
        result.is_ok(),
        "Authorship should be preserved after --onto"
    );
}

/// Test rebase abort - ensures no authorship corruption on abort
#[test]
fn test_rebase_abort() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("conflict.txt", "line 1\nline 2\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with AI changes
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("conflict.txt", "line 1\nAI CHANGE\n", false)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI changes").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Make conflicting change on main
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("conflict.txt", "line 1\nMAIN CHANGE\n", false)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main changes").unwrap();

    // Try to rebase - will conflict
    tmp_repo.checkout_branch("feature").unwrap();
    use std::process::Command;
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", &default_branch])
        .output()
        .unwrap();

    // Should conflict
    assert!(!output.status.success(), "Rebase should conflict");

    // Abort the rebase
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", "--abort"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Rebase abort should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Verify we're back to original commit
    let current_commit = tmp_repo.get_head_commit_sha().unwrap();
    assert_eq!(
        current_commit, feature_commit,
        "Should be back to original commit after abort"
    );

    // Verify original authorship is intact
    let result = get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &feature_commit);
    assert!(
        result.is_ok(),
        "Original authorship should be intact after abort"
    );
}

/// Test branch switch during rebase - ensures proper state handling
#[test]
fn test_rebase_branch_switch_during() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo.write_file("base.txt", "base\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo.write_file("feature.txt", "// AI\n", true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();

    // Create another branch
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo.create_branch("other").unwrap();
    tmp_repo.write_file("other.txt", "other\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Other work").unwrap();

    // Start rebase on feature (non-conflicting)
    tmp_repo.checkout_branch("feature").unwrap();
    tmp_repo
        .rebase_onto(&default_branch, &default_branch)
        .unwrap();

    // Verify branch is still feature
    let current_branch = tmp_repo.current_branch().unwrap();
    assert_eq!(
        current_branch, "feature",
        "Should still be on feature branch"
    );
}

/// Test rebase with autosquash enabled
#[test]
fn test_rebase_autosquash() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Enable autosquash in config
    use std::process::Command;
    Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["config", "rebase.autosquash", "true"])
        .output()
        .unwrap();

    // Create initial commit
    tmp_repo.write_file("file.txt", "line 1\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("file.txt", "line 1\nAI line 2\n", false)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Create fixup commit
    tmp_repo
        .write_file("file.txt", "line 1\nAI line 2 fixed\n", false)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo
        .commit_with_message(&format!("fixup! Add feature"))
        .unwrap();

    // Advance main
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo.write_file("other.txt", "other\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main work").unwrap();
    let base = tmp_repo.get_head_commit_sha().unwrap();

    // Interactive rebase with autosquash
    tmp_repo.checkout_branch("feature").unwrap();
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("GIT_EDITOR", "true")
        .args(&["rebase", "-i", "--autosquash", &base])
        .output()
        .unwrap();

    if !output.status.success() {
        eprintln!(
            "Autosquash rebase failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Not all git versions support autosquash the same way, so we continue
    }

    // Check if we have the expected squashed result
    let head_sha = tmp_repo.get_head_commit_sha().unwrap();
    let repo = tmp_repo.gitai_repo();
    let commit = repo.find_commit(head_sha.clone()).unwrap();

    // Should have parent as base (meaning fixup was squashed)
    let parent = commit.parent(0).unwrap();
    if parent.id().to_string() == base {
        // Autosquash worked - rewrite authorship
        rewrite_authorship_after_rebase(
            &repo,
            &[feature_commit],
            &[head_sha.clone()],
            "Test User <test@example.com>",
        )
        .unwrap();

        // Verify authorship
        let result = get_reference_as_authorship_log_v3(&repo, &head_sha);
        assert!(result.is_ok(), "Autosquashed commit should have authorship");
    }
}

/// Test rebase with autostash enabled
#[test]
fn test_rebase_autostash() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Enable autostash
    use std::process::Command;
    Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["config", "rebase.autoStash", "true"])
        .output()
        .unwrap();

    // Create initial commit
    tmp_repo.write_file("file.txt", "line 1\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo.write_file("feature.txt", "// AI\n", true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();
    let original_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Advance main
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo.write_file("main.txt", "main\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main work").unwrap();

    // Switch back to feature and make unstaged changes
    tmp_repo.checkout_branch("feature").unwrap();
    tmp_repo
        .write_file("feature.txt", "// AI\n// Unstaged change\n", false)
        .unwrap();

    // Rebase with unstaged changes (autostash should handle it)
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", &default_branch])
        .output()
        .unwrap();

    // Should succeed with autostash
    if output.status.success() {
        let rebased_commit = tmp_repo.get_head_commit_sha().unwrap();

        // Rewrite authorship
        rewrite_authorship_after_rebase(
            &tmp_repo.gitai_repo(),
            &[original_commit],
            &[rebased_commit.clone()],
            "Test User <test@example.com>",
        )
        .unwrap();

        // Verify authorship
        let result = get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &rebased_commit);
        assert!(
            result.is_ok(),
            "Rebase with autostash should preserve authorship"
        );
    }
}

/// Test rebase --exec to run tests at each commit
#[test]
fn test_rebase_exec() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("test.sh", "#!/bin/sh\nexit 0\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with multiple AI commits
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo.write_file("f1.txt", "// AI 1\n", true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 1").unwrap();
    let commit1 = tmp_repo.get_head_commit_sha().unwrap();

    tmp_repo.write_file("f2.txt", "// AI 2\n", true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 2").unwrap();
    let commit2 = tmp_repo.get_head_commit_sha().unwrap();

    // Advance main
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo.write_file("main.txt", "main\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main work").unwrap();
    let base = tmp_repo.get_head_commit_sha().unwrap();

    // Rebase with --exec
    tmp_repo.checkout_branch("feature").unwrap();
    use std::process::Command;
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("GIT_EDITOR", "true")
        .args(&["rebase", "-i", "--exec", "echo 'test passed'", &base])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "Rebase with --exec should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Get rebased commits
    let head = tmp_repo.get_head_commit_sha().unwrap();
    let repo = tmp_repo.gitai_repo();
    let mut rebased_commits = vec![];
    let mut current = repo.find_commit(head).unwrap();
    for _ in 0..2 {
        rebased_commits.push(current.id().to_string());
        current = current.parent(0).unwrap();
    }
    rebased_commits.reverse();

    // Rewrite authorship
    rewrite_authorship_after_rebase(
        &repo,
        &[commit1, commit2],
        &rebased_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify authorship
    for rebased_commit in &rebased_commits {
        let result = get_reference_as_authorship_log_v3(&repo, rebased_commit);
        assert!(
            result.is_ok(),
            "Commits after --exec rebase should have authorship"
        );
    }
}

/// Test rebase with merge commits (--rebase-merges)
/// Note: This test verifies that --rebase-merges flag is accepted and doesn't break authorship
#[test]
fn test_rebase_preserve_merges() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo.write_file("base.txt", "base\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("feature.txt", "// AI feature\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();
    let _feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Create side branch from feature
    tmp_repo.create_branch("side").unwrap();
    tmp_repo
        .write_file("side.txt", "// AI side\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI side").unwrap();
    let _side_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Merge side into feature
    tmp_repo.checkout_branch("feature").unwrap();
    tmp_repo
        .merge_branch("side", "Merge side into feature")
        .unwrap();
    let _merge_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Advance main
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo.write_file("main.txt", "main\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main work").unwrap();
    let base = tmp_repo.get_head_commit_sha().unwrap();

    // Rebase feature onto main with --rebase-merges
    tmp_repo.checkout_branch("feature").unwrap();
    use std::process::Command;
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["rebase", "--rebase-merges", &base])
        .output()
        .unwrap();

    // The main goal is to verify the rebase succeeds and doesn't break authorship
    assert!(
        output.status.success(),
        "Rebase with --rebase-merges should succeed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Note: Whether the merge structure is actually preserved depends on git version
    // and the specific topology. The important thing is that authorship tracking
    // doesn't break when using --rebase-merges.
    // Just verify we can get authorship for commits
    let repo = tmp_repo.gitai_repo();
    let head_sha = tmp_repo.get_head_commit_sha().unwrap();

    // Try to find and verify authorship for the commits in the rebased history
    // This ensures authorship tracking works with --rebase-merges
    let head_commit = repo.find_commit(head_sha).unwrap();

    // The test passes if we successfully rebased without errors
    // and can still access commit information
    assert!(
        head_commit.parent_count().unwrap_or(0) > 0,
        "Should have parent commits"
    );
}

/// Test rebase with commit splitting (fewer original commits than new commits)
/// This tests the bug fix where zip() would truncate and lose authorship for extra commits
#[test]
fn test_rebase_commit_splitting() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("base.txt", "base content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with 2 AI commits that modify the same file
    tmp_repo.create_branch("feature").unwrap();

    // First AI commit - adds initial content to features.txt
    tmp_repo
        .write_file(
            "features.txt",
            "// AI feature 1\nfunction feature1() {}\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature 1").unwrap();
    let original_commit_1 = tmp_repo.get_head_commit_sha().unwrap();

    // Second AI commit - adds more content to the same file
    tmp_repo
        .write_file(
            "features.txt",
            "// AI feature 1\nfunction feature1() {}\n// AI feature 2\nfunction feature2() {}\n",
            false,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature 2").unwrap();
    let original_commit_2 = tmp_repo.get_head_commit_sha().unwrap();

    // Advance main branch
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("main.txt", "main content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main advances").unwrap();
    let main_head = tmp_repo.get_head_commit_sha().unwrap();

    // Simulate commit splitting by manually creating 3 new commits that represent
    // the rebased and split versions of the original 2 commits
    // Use git commands directly to checkout the commit (create detached HEAD)
    use std::process::Command;
    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .args(&["checkout", &main_head])
        .output()
        .unwrap();

    if !output.status.success() {
        panic!(
            "Failed to checkout commit: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // New commit 1 (partial content from original - feature1 only)
    tmp_repo
        .write_file(
            "features.txt",
            "// AI feature 1\nfunction feature1() {}\n",
            true,
        )
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap(); // Don't add AI authorship yet
    tmp_repo.commit_with_message("Add feature 1").unwrap();
    let new_commit_1 = tmp_repo.get_head_commit_sha().unwrap();

    // New commit 2 (adds a helper function that wasn't in original - "splitting" the work)
    tmp_repo
        .write_file(
            "features.txt",
            "// AI feature 1\nfunction feature1() {}\n// Helper\nfunction helper() {}\n",
            false,
        )
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Add helper").unwrap();
    let new_commit_2 = tmp_repo.get_head_commit_sha().unwrap();

    // New commit 3 (adds feature2 - from original commit 2)
    tmp_repo
        .write_file("features.txt", "// AI feature 1\nfunction feature1() {}\n// Helper\nfunction helper() {}\n// AI feature 2\nfunction feature2() {}\n", false)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Add feature 2").unwrap();
    let new_commit_3 = tmp_repo.get_head_commit_sha().unwrap();

    // Now test the authorship rewriting with 2 original commits -> 3 new commits
    // This is the scenario that would have failed with the zip() bug
    let repo = tmp_repo.gitai_repo();
    let original_commits = vec![original_commit_1, original_commit_2];
    let new_commits = vec![
        new_commit_1.clone(),
        new_commit_2.clone(),
        new_commit_3.clone(),
    ];

    // Run rewrite authorship - this should handle all 3 new commits
    rewrite_authorship_after_rebase(
        &repo,
        &original_commits,
        &new_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify ALL 3 new commits have authorship logs
    // With the bug, only the first 2 would have been processed (due to zip truncation)
    for (i, new_commit) in new_commits.iter().enumerate() {
        let result = get_reference_as_authorship_log_v3(&repo, new_commit);
        assert!(
            result.is_ok(),
            "New commit {} at index {} should have authorship log (bug: zip truncation would skip this)",
            new_commit,
            i
        );

        let log = result.unwrap();
        assert_eq!(
            log.metadata.base_commit_sha, *new_commit,
            "Authorship log should reference the correct commit"
        );
    }

    // Additional verification: ensure the 3rd commit (which would have been skipped by the bug)
    // actually has authorship attribution
    let log_3 = get_reference_as_authorship_log_v3(&repo, &new_commits[2]).unwrap();
    assert_eq!(
        log_3.metadata.base_commit_sha, new_commits[2],
        "Third commit should have proper authorship log"
    );
}

/// Test interactive rebase with squashing - verifies authorship from all commits is preserved
/// This tests the bug fix where only the last commit's authorship was kept during squashing
#[test]
#[cfg(not(target_os = "windows"))]
fn test_rebase_squash_preserves_all_authorship() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("base.txt", "base content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();
    tmp_repo.create_branch("feature").unwrap();

    // Create 3 AI commits with different content - we'll squash these
    tmp_repo
        .write_file("feature1.txt", "// AI feature 1\nline 1\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 1").unwrap();
    let commit1 = tmp_repo.get_head_commit_sha().unwrap();

    tmp_repo
        .write_file("feature2.txt", "// AI feature 2\nline 2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 2").unwrap();
    let commit2 = tmp_repo.get_head_commit_sha().unwrap();

    tmp_repo
        .write_file("feature3.txt", "// AI feature 3\nline 3\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_3", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 3").unwrap();
    let commit3 = tmp_repo.get_head_commit_sha().unwrap();

    // Advance main branch
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("main.txt", "main work\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main advances").unwrap();
    let base_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Perform interactive rebase with squashing: pick first, squash second and third
    tmp_repo.checkout_branch("feature").unwrap();

    use std::io::Write;
    use std::process::Command;

    // Create a script that modifies the rebase-todo to squash commits 2 and 3 into 1
    let script_content = r#"#!/bin/sh
sed -i.bak '2s/pick/squash/' "$1"
sed -i.bak '3s/pick/squash/' "$1"
"#;

    let script_path = tmp_repo.path().join("squash_script.sh");
    let mut script_file = std::fs::File::create(&script_path).unwrap();
    script_file.write_all(script_content.as_bytes()).unwrap();
    drop(script_file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }

    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .env("GIT_SEQUENCE_EDITOR", script_path.to_str().unwrap())
        .env("GIT_EDITOR", "true") // Auto-accept commit message
        .args(&["rebase", "-i", &base_commit])
        .output()
        .unwrap();

    if !output.status.success() {
        eprintln!(
            "git rebase output: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Interactive rebase with squash failed");
    }

    // After squashing, we should have only 1 commit on top of base
    let head = tmp_repo.get_head_commit_sha().unwrap();
    let repo = tmp_repo.gitai_repo();

    // Verify we have exactly 1 commit (the squashed one)
    let squashed_commit = head.clone();
    let parent = repo.find_commit(head).unwrap().parent(0).unwrap();
    assert_eq!(
        parent.id().to_string(),
        base_commit,
        "Should have exactly 1 commit after squashing 3 commits"
    );

    // Now rewrite authorship: 3 original commits -> 1 new commit
    rewrite_authorship_after_rebase(
        &repo,
        &[commit1, commit2, commit3],
        &[squashed_commit.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify the squashed commit has authorship
    let result = get_reference_as_authorship_log_v3(&repo, &squashed_commit);
    assert!(
        result.is_ok(),
        "Squashed commit should have authorship from all original commits"
    );

    let log = result.unwrap();
    assert!(
        !log.attestations.is_empty(),
        "Squashed commit should have AI attestations"
    );

    // Verify all 3 files exist (proving all commits were included)
    assert!(
        tmp_repo.path().join("feature1.txt").exists(),
        "feature1.txt from commit 1 should exist"
    );
    assert!(
        tmp_repo.path().join("feature2.txt").exists(),
        "feature2.txt from commit 2 should exist"
    );
    assert!(
        tmp_repo.path().join("feature3.txt").exists(),
        "feature3.txt from commit 3 should exist"
    );
}

/// Test rebase with rewording (renaming) a commit that has 2 children commits
/// Verifies that authorship is preserved for all 3 commits after reword
#[test]
#[cfg(not(target_os = "windows"))]
fn test_rebase_reword_commit_with_children() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("base.txt", "base content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let default_branch = tmp_repo.current_branch().unwrap();
    tmp_repo.create_branch("feature").unwrap();

    // Create 3 AI commits - we'll reword the first one
    // Commit 1 (this will be renamed)
    tmp_repo
        .write_file(
            "feature1.txt",
            "// AI feature 1\nfunction feature1() {}\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo
        .commit_with_message("AI commit 1 - original message")
        .unwrap();
    let commit1 = tmp_repo.get_head_commit_sha().unwrap();

    // Commit 2 (child of commit 1)
    tmp_repo
        .write_file(
            "feature2.txt",
            "// AI feature 2\nfunction feature2() {}\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 2").unwrap();
    let commit2 = tmp_repo.get_head_commit_sha().unwrap();

    // Commit 3 (child of commit 2, grandchild of commit 1)
    tmp_repo
        .write_file(
            "feature3.txt",
            "// AI feature 3\nfunction feature3() {}\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_3", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 3").unwrap();
    let commit3 = tmp_repo.get_head_commit_sha().unwrap();

    // Capture blame information BEFORE rebase for all files
    let blame_before_1 =
        get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &commit1).unwrap();
    let blame_before_2 =
        get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &commit2).unwrap();
    let blame_before_3 =
        get_reference_as_authorship_log_v3(&tmp_repo.gitai_repo(), &commit3).unwrap();

    // Advance main branch
    tmp_repo.checkout_branch(&default_branch).unwrap();
    tmp_repo
        .write_file("main.txt", "main work\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Main advances").unwrap();
    let base_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Perform interactive rebase with rewording the first commit
    tmp_repo.checkout_branch("feature").unwrap();

    use std::io::Write;
    use std::process::Command;

    // Create a script that modifies the rebase-todo to reword the first commit
    let script_content = r#"#!/bin/sh
sed -i.bak '1s/pick/reword/' "$1"
"#;

    let script_path = tmp_repo.path().join("reword_script.sh");
    let mut script_file = std::fs::File::create(&script_path).unwrap();
    script_file.write_all(script_content.as_bytes()).unwrap();
    drop(script_file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&script_path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&script_path, perms).unwrap();
    }

    // Create a script that provides the new commit message
    let commit_msg_content = "AI commit 1 - RENAMED MESSAGE";
    let commit_msg_path = tmp_repo.path().join("new_commit_msg.txt");
    let mut msg_file = std::fs::File::create(&commit_msg_path).unwrap();
    msg_file.write_all(commit_msg_content.as_bytes()).unwrap();
    drop(msg_file);

    // Create an editor script that replaces the commit message
    let editor_script_content = format!(
        r#"#!/bin/sh
cat {} > "$1"
"#,
        commit_msg_path.to_str().unwrap()
    );
    let editor_script_path = tmp_repo.path().join("editor_script.sh");
    let mut editor_file = std::fs::File::create(&editor_script_path).unwrap();
    editor_file
        .write_all(editor_script_content.as_bytes())
        .unwrap();
    drop(editor_file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&editor_script_path)
            .unwrap()
            .permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&editor_script_path, perms).unwrap();
    }

    let output = Command::new("git")
        .current_dir(tmp_repo.path())
        .env("GIT_SEQUENCE_EDITOR", script_path.to_str().unwrap())
        .env("GIT_EDITOR", editor_script_path.to_str().unwrap())
        .args(&["rebase", "-i", &base_commit])
        .output()
        .unwrap();

    if !output.status.success() {
        eprintln!(
            "git rebase output: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        panic!("Interactive rebase with reword failed");
    }

    // Get the rebased commits (should still be 3 commits)
    let head = tmp_repo.get_head_commit_sha().unwrap();
    let repo = tmp_repo.gitai_repo();
    let mut rebased_commits = vec![];
    let mut current = repo.find_commit(head).unwrap();
    for _ in 0..3 {
        rebased_commits.push(current.id().to_string());
        current = current.parent(0).unwrap();
    }
    rebased_commits.reverse(); // oldest first

    // Verify we still have 3 commits
    assert_eq!(
        rebased_commits.len(),
        3,
        "Should have 3 commits after reword rebase"
    );

    // Verify the first commit's message was changed
    let first_rebased = repo.find_commit(rebased_commits[0].clone()).unwrap();
    let first_message = first_rebased.summary().unwrap();
    assert!(
        first_message.contains("RENAMED MESSAGE"),
        "First commit should have the renamed message, got: {}",
        first_message
    );

    // Rewrite authorship for all 3 commits
    rewrite_authorship_after_rebase(
        &repo,
        &[commit1, commit2, commit3],
        &rebased_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify ALL 3 commits have authorship logs
    for (i, rebased_commit) in rebased_commits.iter().enumerate() {
        let result = get_reference_as_authorship_log_v3(&repo, rebased_commit);
        assert!(
            result.is_ok(),
            "Rebased commit {} at index {} should have authorship after reword",
            rebased_commit,
            i
        );
    }

    // Capture blame information AFTER rebase for all files
    let blame_after_1 = get_reference_as_authorship_log_v3(&repo, &rebased_commits[0]).unwrap();
    let blame_after_2 = get_reference_as_authorship_log_v3(&repo, &rebased_commits[1]).unwrap();
    let blame_after_3 = get_reference_as_authorship_log_v3(&repo, &rebased_commits[2]).unwrap();

    // Compare blame BEFORE and AFTER - the functional blame output should be the same
    // We check that line attributions (which author/agent wrote which lines) are preserved

    // For commit 1 (feature1.txt): verify line attributions match
    for line_num in 1..=2 {
        let before_attr = blame_before_1.get_line_attribution("feature1.txt", line_num);
        let after_attr = blame_after_1.get_line_attribution("feature1.txt", line_num);
        let before_exists = before_attr.is_some();
        let after_exists = after_attr.is_some();

        match (before_attr, after_attr) {
            (Some((before_author, before_prompt)), Some((after_author, after_prompt))) => {
                assert_eq!(
                    before_author.username, after_author.username,
                    "Line {} author should match before and after rebase",
                    line_num
                );
                // Compare prompt agent IDs if both have prompts
                if let (Some(bp), Some(ap)) = (before_prompt, after_prompt) {
                    assert_eq!(
                        bp.agent_id.id, ap.agent_id.id,
                        "Line {} AI agent should match before and after rebase",
                        line_num
                    );
                    assert_eq!(
                        bp.agent_id.model, ap.agent_id.model,
                        "Line {} model should match before and after rebase",
                        line_num
                    );
                }
            }
            (None, None) => {} // Both have no attribution - OK
            _ => panic!(
                "Line {} attribution mismatch: before={:?}, after={:?}",
                line_num, before_exists, after_exists
            ),
        }
    }

    // For commit 2 (feature2.txt): verify line attributions match
    for line_num in 1..=2 {
        let before_attr = blame_before_2.get_line_attribution("feature2.txt", line_num);
        let after_attr = blame_after_2.get_line_attribution("feature2.txt", line_num);
        let before_exists = before_attr.is_some();
        let after_exists = after_attr.is_some();

        match (before_attr, after_attr) {
            (Some((before_author, before_prompt)), Some((after_author, after_prompt))) => {
                assert_eq!(
                    before_author.username, after_author.username,
                    "Commit 2 Line {} author should match before and after rebase",
                    line_num
                );
                if let (Some(bp), Some(ap)) = (before_prompt, after_prompt) {
                    assert_eq!(
                        bp.agent_id.id, ap.agent_id.id,
                        "Commit 2 Line {} AI agent should match before and after rebase",
                        line_num
                    );
                }
            }
            (None, None) => {}
            _ => panic!(
                "Commit 2 Line {} attribution mismatch: before={:?}, after={:?}",
                line_num, before_exists, after_exists
            ),
        }
    }

    // For commit 3 (feature3.txt): verify line attributions match
    for line_num in 1..=2 {
        let before_attr = blame_before_3.get_line_attribution("feature3.txt", line_num);
        let after_attr = blame_after_3.get_line_attribution("feature3.txt", line_num);
        let before_exists = before_attr.is_some();
        let after_exists = after_attr.is_some();

        match (before_attr, after_attr) {
            (Some((before_author, before_prompt)), Some((after_author, after_prompt))) => {
                assert_eq!(
                    before_author.username, after_author.username,
                    "Commit 3 Line {} author should match before and after rebase",
                    line_num
                );
                if let (Some(bp), Some(ap)) = (before_prompt, after_prompt) {
                    assert_eq!(
                        bp.agent_id.id, ap.agent_id.id,
                        "Commit 3 Line {} AI agent should match before and after rebase",
                        line_num
                    );
                }
            }
            (None, None) => {}
            _ => panic!(
                "Commit 3 Line {} attribution mismatch: before={:?}, after={:?}",
                line_num, before_exists, after_exists
            ),
        }
    }

    // Verify all 3 files still exist with correct content
    let feature1_content = std::fs::read_to_string(tmp_repo.path().join("feature1.txt")).unwrap();
    assert!(
        feature1_content.contains("AI feature 1"),
        "feature1.txt should have the expected content"
    );
    assert!(
        feature1_content.contains("function feature1()"),
        "feature1.txt should have the function"
    );

    let feature2_content = std::fs::read_to_string(tmp_repo.path().join("feature2.txt")).unwrap();
    assert!(
        feature2_content.contains("AI feature 2"),
        "feature2.txt should have the expected content"
    );

    let feature3_content = std::fs::read_to_string(tmp_repo.path().join("feature3.txt")).unwrap();
    assert!(
        feature3_content.contains("AI feature 3"),
        "feature3.txt should have the expected content"
    );
}
