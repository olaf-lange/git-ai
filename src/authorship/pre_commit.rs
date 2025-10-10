use crate::error::GitAiError;
use crate::git::repository::Repository;

pub fn pre_commit(repo: &Repository, default_author: String) -> Result<(), GitAiError> {
    // Run checkpoint as human editor.
    let result =
        crate::commands::checkpoint::run(repo, &default_author, false, false, true, None, true);
    result.map(|_| ())
}
