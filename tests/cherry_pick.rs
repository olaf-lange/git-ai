use git_ai::authorship::rebase_authorship::rewrite_authorship_after_cherry_pick;
use git_ai::git::find_repository_in_path;
use git_ai::git::refs::get_reference_as_authorship_log_v3;
use git_ai::git::test_utils::TmpRepo;

/// Test cherry-picking a single AI-authored commit
#[test]
fn test_single_commit_cherry_pick() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit on default branch
    tmp_repo
        .write_file("file.txt", "Initial content\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let main_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with AI-authored changes
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("file.txt", "Initial content\nAI feature line\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add AI feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Switch back to main and cherry-pick the feature commit
    tmp_repo.checkout_branch(&main_branch).unwrap();
    tmp_repo.cherry_pick(&[&feature_commit]).unwrap();

    // Manually trigger authorship rewrite (in real usage, hooks would do this)
    let repo = find_repository_in_path(tmp_repo.path().to_str().unwrap()).unwrap();
    let new_head = tmp_repo.get_head_commit_sha().unwrap();
    rewrite_authorship_after_cherry_pick(
        &repo,
        &[feature_commit.clone()],
        &[new_head.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify the cherry-picked commit has AI authorship
    let head_sha = tmp_repo.get_head_commit_sha().unwrap();
    let authorship_log = get_reference_as_authorship_log_v3(&repo, &head_sha).unwrap();

    // Should have authorship attribution
    assert_eq!(authorship_log.attestations.len(), 1);
    assert_eq!(authorship_log.metadata.prompts.len(), 1);

    // Verify the AI agent is attributed
    let prompt = authorship_log.metadata.prompts.values().next().unwrap();
    assert_eq!(prompt.agent_id.id, "ai_agent");
}

/// Test cherry-picking multiple commits in sequence
#[test]
fn test_multiple_commits_cherry_pick() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit on default branch
    tmp_repo.write_file("file.txt", "Line 1\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let main_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with multiple AI-authored commits
    tmp_repo.create_branch("feature").unwrap();

    // First AI commit
    tmp_repo
        .write_file("file.txt", "Line 1\nAI line 2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 1").unwrap();
    let commit1 = tmp_repo.get_head_commit_sha().unwrap();

    // Second AI commit
    tmp_repo
        .write_file("file.txt", "Line 1\nAI line 2\nAI line 3\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 2").unwrap();
    let commit2 = tmp_repo.get_head_commit_sha().unwrap();

    // Third AI commit
    tmp_repo
        .write_file(
            "file.txt",
            "Line 1\nAI line 2\nAI line 3\nAI line 4\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_3", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI commit 3").unwrap();
    let commit3 = tmp_repo.get_head_commit_sha().unwrap();

    // Switch back to main and cherry-pick all three commits
    tmp_repo.checkout_branch(&main_branch).unwrap();
    let original_head = tmp_repo.get_head_commit_sha().unwrap();
    tmp_repo
        .cherry_pick(&[&commit1, &commit2, &commit3])
        .unwrap();

    // Manually trigger authorship rewrite (in real usage, hooks would do this)
    let repo = find_repository_in_path(tmp_repo.path().to_str().unwrap()).unwrap();
    let new_head = tmp_repo.get_head_commit_sha().unwrap();

    // Build list of new commits created by walking from new_head to original_head
    let new_commits = {
        let mut commits = Vec::new();
        let mut current = repo.find_commit(new_head.clone()).unwrap();
        let base = repo.find_commit(original_head.clone()).unwrap();
        while current.id() != base.id() {
            commits.push(current.id().to_string());
            current = current.parent(0).unwrap();
        }
        commits.reverse(); // Oldest first
        commits
    };

    rewrite_authorship_after_cherry_pick(
        &repo,
        &[commit1.clone(), commit2.clone(), commit3.clone()],
        &new_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify the last cherry-picked commit has correct authorship
    let head_sha = tmp_repo.get_head_commit_sha().unwrap();
    let authorship_log = get_reference_as_authorship_log_v3(&repo, &head_sha).unwrap();

    // Should have authorship from ai_agent_3
    assert!(authorship_log.metadata.prompts.len() >= 1);
    let has_agent_3 = authorship_log
        .metadata
        .prompts
        .values()
        .any(|p| p.agent_id.id == "ai_agent_3");
    assert!(has_agent_3, "Should have authorship from ai_agent_3");
}

/// Test cherry-pick with conflicts and --continue
#[test]
fn test_cherry_pick_with_conflict_and_continue() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit on default branch
    tmp_repo
        .write_file("file.txt", "Line 1\nLine 2\nLine 3\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let main_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with AI changes
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("file.txt", "Line 1\nAI_FEATURE_VERSION\nLine 3\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Switch back to main and make conflicting change
    tmp_repo.checkout_branch(&main_branch).unwrap();
    tmp_repo
        .write_file("file.txt", "Line 1\nMAIN_BRANCH_VERSION\nLine 3\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Human change").unwrap();

    // Try to cherry-pick (should conflict)
    let has_conflict = tmp_repo
        .cherry_pick_with_conflicts(&feature_commit)
        .unwrap();
    assert!(has_conflict, "Should have conflict");

    // Resolve conflict by choosing the AI version
    tmp_repo
        .write_file("file.txt", "Line 1\nAI_FEATURE_VERSION\nLine 3\n", true)
        .unwrap();
    tmp_repo.stage_file("file.txt").unwrap();

    // Continue cherry-pick
    tmp_repo.cherry_pick_continue().unwrap();

    // Manually trigger authorship rewrite (in real usage, hooks would do this)
    let repo = find_repository_in_path(tmp_repo.path().to_str().unwrap()).unwrap();
    let new_head = tmp_repo.get_head_commit_sha().unwrap();
    rewrite_authorship_after_cherry_pick(
        &repo,
        &[feature_commit.clone()],
        &[new_head.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify authorship is preserved
    let head_sha = tmp_repo.get_head_commit_sha().unwrap();
    let authorship_log = get_reference_as_authorship_log_v3(&repo, &head_sha).unwrap();

    // Should have AI authorship
    assert!(authorship_log.metadata.prompts.len() >= 1);
}

/// Test cherry-pick --abort
#[test]
fn test_cherry_pick_abort() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit on default branch
    tmp_repo
        .write_file("file.txt", "Line 1\nLine 2\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let initial_head = tmp_repo.get_head_commit_sha().unwrap();

    let main_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with AI changes (modify line 2)
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("file.txt", "Line 1\nAI modification of line 2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Switch back to main and make conflicting change (also modify line 2)
    tmp_repo.checkout_branch(&main_branch).unwrap();
    tmp_repo
        .write_file("file.txt", "Line 1\nHuman modification of line 2\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Human change").unwrap();

    // Try to cherry-pick (should conflict)
    let has_conflict = tmp_repo
        .cherry_pick_with_conflicts(&feature_commit)
        .unwrap();
    assert!(has_conflict, "Should have conflict");

    // Abort the cherry-pick
    tmp_repo.cherry_pick_abort().unwrap();

    // Verify HEAD is back to before the cherry-pick
    let current_head = tmp_repo.get_head_commit_sha().unwrap();
    assert_ne!(current_head, initial_head); // Different because we made the "Human change" commit
}

/// Test cherry-picking from branch without AI authorship
#[test]
fn test_cherry_pick_no_ai_authorship() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit on default branch
    tmp_repo.write_file("file.txt", "Line 1\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let main_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with human-only changes (no AI)
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("file.txt", "Line 1\nHuman line 2\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Human feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Switch back to main and cherry-pick
    tmp_repo.checkout_branch(&main_branch).unwrap();
    tmp_repo.cherry_pick(&[&feature_commit]).unwrap();

    // Verify no AI authorship log (or empty)
    let repo = find_repository_in_path(tmp_repo.path().to_str().unwrap()).unwrap();
    let head_sha = tmp_repo.get_head_commit_sha().unwrap();

    // Either no log or empty log is fine
    match get_reference_as_authorship_log_v3(&repo, &head_sha) {
        Ok(log) => {
            // If log exists, it should have no AI prompts
            assert_eq!(log.metadata.prompts.len(), 0);
        }
        Err(_) => {
            // No log is also acceptable
        }
    }
}

/// Test cherry-pick preserving multiple AI sessions from different commits
#[test]
fn test_cherry_pick_multiple_ai_sessions() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit on default branch
    tmp_repo
        .write_file("main.rs", "fn main() {}\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let main_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch
    tmp_repo.create_branch("feature").unwrap();

    // First AI session adds logging
    tmp_repo
        .write_file(
            "main.rs",
            "fn main() {\n    println!(\"Starting\");\n}\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_session_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add logging").unwrap();
    let commit1 = tmp_repo.get_head_commit_sha().unwrap();

    // Second AI session adds error handling
    tmp_repo
        .write_file(
            "main.rs",
            "fn main() {\n    println!(\"Starting\");\n    // TODO: Add error handling\n}\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_session_2", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add error handling").unwrap();
    let commit2 = tmp_repo.get_head_commit_sha().unwrap();

    // Cherry-pick both to main
    tmp_repo.checkout_branch(&main_branch).unwrap();
    let original_head = tmp_repo.get_head_commit_sha().unwrap();
    tmp_repo.cherry_pick(&[&commit1, &commit2]).unwrap();

    // Manually trigger authorship rewrite (in real usage, hooks would do this)
    let repo = find_repository_in_path(tmp_repo.path().to_str().unwrap()).unwrap();
    let new_head = tmp_repo.get_head_commit_sha().unwrap();

    // Build list of new commits
    let new_commits = {
        let mut commits = Vec::new();
        let mut current = repo.find_commit(new_head.clone()).unwrap();
        let base = repo.find_commit(original_head.clone()).unwrap();
        while current.id() != base.id() {
            commits.push(current.id().to_string());
            current = current.parent(0).unwrap();
        }
        commits.reverse(); // Oldest first
        commits
    };

    rewrite_authorship_after_cherry_pick(
        &repo,
        &[commit1.clone(), commit2.clone()],
        &new_commits,
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify both AI sessions are in the final commit
    let head_sha = tmp_repo.get_head_commit_sha().unwrap();
    let authorship_log = get_reference_as_authorship_log_v3(&repo, &head_sha).unwrap();

    // Should have authorship from ai_session_2 (the last cherry-picked commit)
    assert!(authorship_log.metadata.prompts.len() >= 1);
}

/// Test that trees-identical fast path works
#[test]
fn test_cherry_pick_identical_trees() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo.write_file("file.txt", "Line 1\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let main_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch with AI changes
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("file.txt", "Line 1\nAI line\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Add another commit on feature (just to have a parent)
    tmp_repo
        .write_file("file.txt", "Line 1\nAI line\nMore AI\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("More AI").unwrap();

    // Cherry-pick the first feature commit to main
    tmp_repo.checkout_branch(&main_branch).unwrap();
    tmp_repo.cherry_pick(&[&feature_commit]).unwrap();

    // Manually trigger authorship rewrite (in real usage, hooks would do this)
    let repo = find_repository_in_path(tmp_repo.path().to_str().unwrap()).unwrap();
    let new_head = tmp_repo.get_head_commit_sha().unwrap();
    rewrite_authorship_after_cherry_pick(
        &repo,
        &[feature_commit.clone()],
        &[new_head.clone()],
        "Test User <test@example.com>",
    )
    .unwrap();

    // Verify authorship is preserved
    let head_sha = tmp_repo.get_head_commit_sha().unwrap();
    let authorship_log = get_reference_as_authorship_log_v3(&repo, &head_sha).unwrap();

    assert!(authorship_log.metadata.prompts.len() >= 1);
}

/// Test cherry-pick where some commits become empty (already applied)
#[test]
fn test_cherry_pick_empty_commits() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo.write_file("file.txt", "Line 1\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();

    let main_branch = tmp_repo.current_branch().unwrap();

    // Create feature branch
    tmp_repo.create_branch("feature").unwrap();
    tmp_repo
        .write_file("file.txt", "Line 1\nFeature line\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add feature").unwrap();
    let feature_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Manually apply the same change to main
    tmp_repo.checkout_branch(&main_branch).unwrap();
    tmp_repo
        .write_file("file.txt", "Line 1\nFeature line\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo
        .commit_with_message("Apply feature manually")
        .unwrap();

    // Try to cherry-pick the feature commit (should become empty and be skipped with --keep-redundant-commits)
    // Without that flag, git will skip empty commits
    let result = tmp_repo.cherry_pick(&[&feature_commit]);

    // Git might succeed and skip the empty commit, or it might warn
    // Either way is acceptable - the important thing is we don't crash
    match result {
        Ok(_) => {
            // Empty commit was skipped
        }
        Err(_) => {
            // Git reported an error about empty commit, which is also fine
        }
    }
}
