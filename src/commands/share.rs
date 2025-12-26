use crate::api::{ApiClient, ApiContext};
use crate::api::{BundleData, CreateBundleRequest};
use crate::authorship::prompt_utils::find_prompt_with_db_fallback;
use crate::git::find_repository;
use std::collections::HashMap;

/// Handle the `share` command
///
/// Usage: git-ai share <prompt_id> [--title <title>]
///
/// Shares a prompt by creating a bundle via the API.
pub fn handle_share(args: &[String]) {
    let parsed = match parse_args(args) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Try to find repository (optional - prompt might be in DB)
    let repo = find_repository(&Vec::<String>::new()).ok();

    // Find the prompt (DB first, then repository)
    let (_commit_sha, prompt_record) = match find_prompt_with_db_fallback(&parsed.prompt_id, repo.as_ref()) {
        Ok((sha, prompt)) => (sha, prompt),
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    // Generate a title if not provided
    let title = parsed.title.unwrap_or_else(|| {
        format!(
            "Prompt {} ({})",
            parsed.prompt_id,
            prompt_record.agent_id.tool
        )
    });

    // Create bundle request
    let mut prompts = HashMap::new();
    prompts.insert(parsed.prompt_id.clone(), prompt_record);

    let bundle_request = CreateBundleRequest {
        title,
        data: BundleData {
            prompts,
            files: HashMap::new(),
        },
    };

    // Create API client (uses default URL from env or default)
    let context = ApiContext::new(None);
    let client = ApiClient::new(context);

    // Create the bundle
    match client.create_bundle(bundle_request) {
        Ok(response) => {
            println!("Bundle created successfully!");
            println!("ID: {}", response.id);
            println!("URL: {}", response.url);
        }
        Err(e) => {
            eprintln!("Failed to create bundle: {}", e);
            std::process::exit(1);
        }
    }
}

#[derive(Debug)]
pub struct ParsedArgs {
    pub prompt_id: String,
    pub title: Option<String>,
}

pub fn parse_args(args: &[String]) -> Result<ParsedArgs, String> {
    let mut prompt_id: Option<String> = None;
    let mut title: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];

        if arg == "--title" {
            if i + 1 >= args.len() {
                return Err("--title requires a value".to_string());
            }
            i += 1;
            title = Some(args[i].clone());
        } else if arg.starts_with('-') {
            return Err(format!("Unknown option: {}", arg));
        } else {
            if prompt_id.is_some() {
                return Err("Only one prompt ID can be specified".to_string());
            }
            prompt_id = Some(arg.clone());
        }

        i += 1;
    }

    let prompt_id = prompt_id.ok_or("share requires a prompt ID")?;

    Ok(ParsedArgs { prompt_id, title })
}

