use git_ai::authorship::working_log::{Checkpoint, Line};
use git_ai::git::test_utils::{ResetMode, TmpRepo, snapshot_checkpoints};
use insta::assert_debug_snapshot;

/// Test git reset --hard: should delete working log
#[test]
fn test_reset_hard_deletes_working_log() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("test.txt", "line 1\nline 2\nline 3\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("First commit").unwrap();
    let first_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Make second commit with AI changes
    tmp_repo
        .write_file("test.txt", "line 1\nline 2\nline 3\n// AI line\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Second commit").unwrap();
    let second_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Make some uncommitted AI changes
    tmp_repo
        .write_file(
            "test.txt",
            "line 1\nline 2\nline 3\n// AI line\n// Uncommitted\n",
            false,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Verify working log exists
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&second_commit);
    let checkpoints_before = working_log.read_all_checkpoints().unwrap();
    assert!(
        !checkpoints_before.is_empty(),
        "Working log should exist before reset"
    );

    // Reset --hard to first commit
    tmp_repo.reset(&first_commit, ResetMode::Hard, &[]).unwrap();

    // Verify working log was deleted (directory should not exist or be empty)
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&second_commit);
    let checkpoints_after = working_log.read_all_checkpoints().unwrap();
    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints_after));
}

/// Test git reset --soft: should reconstruct working log from unwound commits
#[test]
fn test_reset_soft_reconstructs_working_log() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("test.txt", "line 1\nline 2\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("First commit").unwrap();
    let first_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Make second commit with AI changes
    tmp_repo
        .write_file("test.txt", "line 1\nline 2\n// AI addition\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Second commit").unwrap();

    // Reset --soft to first commit
    tmp_repo.reset(&first_commit, ResetMode::Soft, &[]).unwrap();

    // Verify working log exists for first commit
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&first_commit);
    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Working log should exist after reset --soft");

    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset --mixed (default): should reconstruct working log
#[test]
fn test_reset_mixed_reconstructs_working_log() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit
    tmp_repo
        .write_file("main.rs", "fn main() {\n}\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial commit").unwrap();
    let first_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Make second commit with AI changes
    tmp_repo
        .write_file(
            "main.rs",
            "fn main() {\n    // AI: Added logging\n    println!(\"Hello\");\n}\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add logging").unwrap();

    // Reset --mixed (or just reset) to first commit
    tmp_repo
        .reset(&first_commit, ResetMode::Mixed, &[])
        .unwrap();

    // Verify working log exists for first commit
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&first_commit);
    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Working log should exist after reset --mixed");

    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset to same commit: should be no-op
#[test]
fn test_reset_to_same_commit_is_noop() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create commit with AI changes
    tmp_repo
        .write_file("test.txt", "line 1\n// AI line\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Commit").unwrap();
    let commit = tmp_repo.get_head_commit_sha().unwrap();

    // Make uncommitted changes
    tmp_repo
        .write_file("test.txt", "line 1\n// AI line\n// More changes\n", false)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Get working log before reset
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&commit);
    let checkpoints_before = working_log.read_all_checkpoints().unwrap();

    // Reset to same commit (HEAD)
    tmp_repo.reset("HEAD", ResetMode::Mixed, &[]).unwrap();

    // Working log should be unchanged
    let checkpoints_after = working_log.read_all_checkpoints().unwrap();
    assert_debug_snapshot!((
        "before",
        snapshot_checkpoints(&checkpoints_before),
        "after",
        snapshot_checkpoints(&checkpoints_after)
    ));
}

/// Test git reset with multiple commits unwound
#[test]
fn test_reset_multiple_commits() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create base commit
    tmp_repo.write_file("code.js", "// Base\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Base").unwrap();
    let base_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Second commit - AI adds feature
    tmp_repo
        .write_file("code.js", "// Base\n// AI feature 1\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_session_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Feature 1").unwrap();

    // Third commit - AI adds another feature
    tmp_repo
        .write_file(
            "code.js",
            "// Base\n// AI feature 1\n// AI feature 2\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_session_2", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Feature 2").unwrap();

    // Reset --soft to base
    tmp_repo.reset(&base_commit, ResetMode::Soft, &[]).unwrap();

    // Verify working log has both AI features
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&base_commit);
    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Working log should exist");

    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset with uncommitted changes preserved
#[test]
fn test_reset_preserves_uncommitted_changes() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create base commit
    tmp_repo
        .write_file("app.py", "def main():\n    pass\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Base").unwrap();
    let base_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Second commit with AI changes
    tmp_repo
        .write_file("app.py", "def main():\n    print('hello')\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add print").unwrap();

    // Third commit with more AI changes
    tmp_repo
        .write_file(
            "app.py",
            "def main():\n    print('hello')\n    print('world')\n",
            true,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add world").unwrap();

    // Reset --soft to base (should preserve both AI commits as staged)
    tmp_repo.reset(&base_commit, ResetMode::Soft, &[]).unwrap();

    // Working log should capture all AI work from both commits
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&base_commit);
    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Working log should exist");

    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset with pathspecs: should remove entries for affected files
#[test]
fn test_reset_with_pathspec() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create initial commit with multiple files
    tmp_repo
        .write_file("file1.txt", "content 1\n", true)
        .unwrap();
    tmp_repo
        .write_file("file2.txt", "content 2\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Initial").unwrap();
    let first_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Commit AI changes to both files
    tmp_repo
        .write_file("file1.txt", "content 1\n// AI change 1\n", true)
        .unwrap();
    tmp_repo
        .write_file("file2.txt", "content 2\n// AI change 2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo
        .commit_with_message("AI changes both files")
        .unwrap();

    // Reset only file1.txt with pathspec (no mode flag, just pathspec)
    // This doesn't move HEAD, just resets index for file1.txt
    // Note: TmpRepo doesn't have a pure pathspec reset, so we'll make uncommitted changes instead
    // and reset HEAD with pathspec to test the filtering

    // First make uncommitted changes to both files
    tmp_repo
        .write_file(
            "file1.txt",
            "content 1\n// AI change 1\n// More AI\n",
            false,
        )
        .unwrap();
    tmp_repo
        .write_file(
            "file2.txt",
            "content 2\n// AI change 2\n// More AI\n",
            false,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Now reset only file1.txt to first commit with pathspec
    tmp_repo
        .reset(&first_commit, ResetMode::Mixed, &["file1.txt"])
        .unwrap();

    // Working log should still have checkpoint for file2.txt only
    // file1.txt should be removed since we reset it
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&first_commit);

    let checkpoints = working_log.read_all_checkpoints().unwrap();
    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset forward (to descendant): should be no-op
#[test]
fn test_reset_forward_is_noop() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create two commits
    tmp_repo.write_file("test.txt", "v1\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("First").unwrap();
    let first_commit = tmp_repo.get_head_commit_sha().unwrap();

    tmp_repo.write_file("test.txt", "v1\nv2\n", true).unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Second").unwrap();
    let second_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Reset back to first
    tmp_repo.reset(&first_commit, ResetMode::Hard, &[]).unwrap();

    // Now reset forward to second (this is a forward reset)
    tmp_repo
        .reset(&second_commit, ResetMode::Soft, &[])
        .unwrap();

    // Working log for second commit should not be created by reset
    // (it already exists from the original commit)
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&second_commit);

    let checkpoints = working_log.read_all_checkpoints().unwrap();
    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset with AI and human mixed changes
#[test]
fn test_reset_mixed_ai_human_changes() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Base commit
    tmp_repo
        .write_file("main.rs", "fn main() {}\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Base").unwrap();
    let base = tmp_repo.get_head_commit_sha().unwrap();

    // AI commit
    tmp_repo
        .write_file("main.rs", "fn main() {\n    // AI\n}\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI changes").unwrap();

    // Human commit
    tmp_repo
        .write_file("main.rs", "fn main() {\n    // AI\n    // Human\n}\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Human changes").unwrap();

    // Reset to base
    tmp_repo.reset(&base, ResetMode::Soft, &[]).unwrap();

    // Working log should exist
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&base);
    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Working log should exist");

    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset --merge
#[test]
fn test_reset_merge() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Create base
    tmp_repo.write_file("test.txt", "base\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Base").unwrap();
    let base = tmp_repo.get_head_commit_sha().unwrap();

    // Create second commit
    tmp_repo
        .write_file("test.txt", "base\n// AI line\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Second").unwrap();

    // Reset --merge
    tmp_repo.reset(&base, ResetMode::Merge, &[]).unwrap();

    // Should reconstruct working log like --mixed
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&base);
    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Working log should exist");

    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset with new files added in unwound commit
#[test]
fn test_reset_with_new_files() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Base commit
    tmp_repo.write_file("old.txt", "existing\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Base").unwrap();
    let base = tmp_repo.get_head_commit_sha().unwrap();

    // Add new file in second commit
    tmp_repo.write_file("old.txt", "existing\n", false).unwrap();
    tmp_repo
        .write_file("new.txt", "// AI created this\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("Add new file").unwrap();

    // Reset to base
    tmp_repo.reset(&base, ResetMode::Soft, &[]).unwrap();

    // Working log should include the new file
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&base);
    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Working log should exist");

    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset with file deletions in unwound commit
#[test]
fn test_reset_with_deleted_files() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Base with two files
    tmp_repo
        .write_file("keep.txt", "keep this\n", true)
        .unwrap();
    tmp_repo
        .write_file("delete.txt", "will delete\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Base").unwrap();
    let base = tmp_repo.get_head_commit_sha().unwrap();

    // Delete one file
    tmp_repo.git_command(&["rm", "delete.txt"]).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Delete file").unwrap();

    // Reset to base
    tmp_repo.reset(&base, ResetMode::Soft, &[]).unwrap();

    // Working log should not error even though file was deleted
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&base);
    let checkpoints = working_log.read_all_checkpoints().unwrap();
    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset --mixed with pathspec: should carry AI authorship forward for reset files
/// and preserve working log for non-reset files
#[test]
fn test_reset_mixed_pathspec_preserves_ai_authorship() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Base commit with two files
    tmp_repo
        .write_file("file1.txt", "base content 1\n", true)
        .unwrap();
    tmp_repo
        .write_file("file2.txt", "base content 2\n", true)
        .unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Base commit").unwrap();
    let base_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Second commit: AI modifies both files
    tmp_repo
        .write_file("file1.txt", "base content 1\n// AI change to file1\n", true)
        .unwrap();
    tmp_repo
        .write_file("file2.txt", "base content 2\n// AI change to file2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo
        .commit_with_message("AI modifies both files")
        .unwrap();
    let second_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Make uncommitted changes to file2 (not file1)
    tmp_repo
        .write_file(
            "file2.txt",
            "base content 2\n// AI change to file2\n// More AI changes\n",
            false,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_agent_2", Some("claude"), Some("cursor"))
        .unwrap();

    // Reset only file1.txt to base commit with pathspec
    // This should carry AI authorship from second_commit for file1.txt
    // and preserve uncommitted changes for file2.txt
    tmp_repo
        .reset(&base_commit, ResetMode::Mixed, &["file1.txt"])
        .unwrap();

    // HEAD should not move (still at second_commit)
    let current_head = tmp_repo.get_head_commit_sha().unwrap();
    assert_eq!(
        current_head, second_commit,
        "HEAD should not move with pathspec reset"
    );

    // Working log should exist for HEAD with:
    // - file1.txt changes from the reset (AI authorship from second_commit reconstructed)
    // - file2.txt uncommitted changes preserved
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&second_commit);
    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Working log should exist after pathspec reset");

    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}

/// Test git reset --mixed with pathspec on multiple commits worth of AI changes
#[test]
fn test_reset_mixed_pathspec_multiple_commits() {
    let tmp_repo = TmpRepo::new().unwrap();

    // Base commit
    tmp_repo.write_file("app.js", "// base\n", true).unwrap();
    tmp_repo.write_file("lib.js", "// base\n", true).unwrap();
    tmp_repo.trigger_checkpoint_with_author("human").unwrap();
    tmp_repo.commit_with_message("Base").unwrap();
    let base_commit = tmp_repo.get_head_commit_sha().unwrap();

    // First AI commit - modifies both files
    tmp_repo
        .write_file("app.js", "// base\n// AI feature 1\n", true)
        .unwrap();
    tmp_repo
        .write_file("lib.js", "// base\n// AI lib 1\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_session_1", Some("gpt-4"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature 1").unwrap();

    // Second AI commit - modifies both files again
    tmp_repo
        .write_file(
            "app.js",
            "// base\n// AI feature 1\n// AI feature 2\n",
            true,
        )
        .unwrap();
    tmp_repo
        .write_file("lib.js", "// base\n// AI lib 1\n// AI lib 2\n", true)
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_session_2", Some("claude"), Some("cursor"))
        .unwrap();
    tmp_repo.commit_with_message("AI feature 2").unwrap();
    let second_ai_commit = tmp_repo.get_head_commit_sha().unwrap();

    // Make uncommitted changes to lib.js (not app.js)
    tmp_repo
        .write_file(
            "lib.js",
            "// base\n// AI lib 1\n// AI lib 2\n// More lib\n",
            false,
        )
        .unwrap();
    tmp_repo
        .trigger_checkpoint_with_ai("ai_session_3", Some("gpt-4"), Some("cursor"))
        .unwrap();

    // Reset only app.js to base with pathspec
    // This should reconstruct AI authorship for app.js from the commits
    // and preserve uncommitted changes for lib.js
    tmp_repo
        .reset(&base_commit, ResetMode::Mixed, &["app.js"])
        .unwrap();

    // HEAD should not move
    let current_head = tmp_repo.get_head_commit_sha().unwrap();
    assert_eq!(current_head, second_ai_commit, "HEAD should not move");

    // Working log should have lib.js with uncommitted AI changes
    // (app.js was reset with pathspec, so no entries for it in working log)
    let working_log = tmp_repo
        .gitai_repo()
        .storage
        .working_log_for_base_commit(&second_ai_commit);
    let checkpoints = working_log
        .read_all_checkpoints()
        .expect("Working log should exist");

    assert_debug_snapshot!(snapshot_checkpoints(&checkpoints));
}
