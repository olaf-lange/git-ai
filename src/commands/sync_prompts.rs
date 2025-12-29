use crate::authorship::authorship_log_serialization::generate_short_hash;
use crate::authorship::internal_db::{InternalDatabase, PromptDbRecord};
use crate::authorship::prompt_utils::{update_prompt_from_tool, PromptUpdateResult};
use crate::commands::checkpoint_agent::agent_presets::{
    ClaudePreset, ContinueCliPreset, CursorPreset, DiscoveredConversation, Discoverable,
};
use crate::error::GitAiError;
use crate::observability::log_error;
use chrono::{DateTime, NaiveDate};
use std::cmp::min;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn handle_sync_prompts(args: &[String]) {
    let mut since: Option<String> = None;
    let mut workdir: Option<String> = None;

    // Parse arguments
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--since" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --since requires a value");
                    std::process::exit(1);
                }
                i += 1;
                since = Some(args[i].clone());
            }
            "--workdir" => {
                if i + 1 >= args.len() {
                    eprintln!("Error: --workdir requires a value");
                    std::process::exit(1);
                }
                i += 1;
                workdir = Some(args[i].clone());
            }
            _ => {
                eprintln!("Error: Unknown argument: {}", args[i]);
                eprintln!("Usage: git-ai sync-prompts [--since <time>] [--workdir <path>]");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    // Parse since into timestamp
    let since_timestamp = if let Some(since_str) = since {
        match parse_since_arg(&since_str) {
            Ok(ts) => Some(ts),
            Err(e) => {
                eprintln!("Error parsing --since: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    // Run sync
    if let Err(e) = sync_prompts(since_timestamp, workdir.as_deref()) {
        eprintln!("Sync failed: {}", e);
        std::process::exit(1);
    }
}

fn parse_since_arg(since_str: &str) -> Result<i64, GitAiError> {
    // Try parsing as relative duration first (1d, 2h, 1w)
    if let Ok(duration) = humantime::parse_duration(since_str) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        return Ok(now - duration.as_secs() as i64);
    }

    // Try parsing as Unix timestamp
    if let Ok(timestamp) = since_str.parse::<i64>() {
        return Ok(timestamp);
    }

    // Try parsing as ISO8601/RFC3339
    if let Ok(dt) = DateTime::parse_from_rfc3339(since_str) {
        return Ok(dt.timestamp());
    }

    // Try parsing as simple date (YYYY-MM-DD)
    if let Ok(dt) = NaiveDate::parse_from_str(since_str, "%Y-%m-%d") {
        let datetime = dt.and_hms_opt(0, 0, 0).unwrap();
        return Ok(datetime.and_utc().timestamp());
    }

    Err(GitAiError::Generic(format!(
        "Invalid --since format: '{}'. Supported formats: '1d', '2h', Unix timestamp, ISO8601, or YYYY-MM-DD",
        since_str
    )))
}

fn sync_prompts(
    since_timestamp: Option<i64>,
    workdir: Option<&str>,
) -> Result<(), GitAiError> {
    eprintln!("Starting prompt sync...");

    let db = InternalDatabase::global()?;
    let mut db_lock = db
        .lock()
        .map_err(|e| GitAiError::Generic(format!("Failed to lock database: {}", e)))?;

    // ===== PHASE 1: UPDATE EXISTING PROMPTS =====
    eprintln!("\n=== Phase 1: Updating existing prompts ===");

    // Query prompts to update (filter by workdir/since if specified)
    let prompts = if let Some(since) = since_timestamp {
        eprintln!("Updating prompts modified since Unix timestamp {}", since);
        db_lock.list_prompts(workdir, Some(since), 10000, 0)?
    } else {
        eprintln!("Updating all prompts in database");
        db_lock.list_prompts(workdir, None, 10000, 0)?
    };

    eprintln!("Found {} prompts to update", prompts.len());

    // Deduplicate by agent_id (keep latest per conversation)
    let prompts_to_update = deduplicate_by_agent_id(&prompts);
    eprintln!(
        "Updating {} unique conversations",
        prompts_to_update.len()
    );

    // Update each prompt (existing logic)
    let mut updated_records = Vec::new();
    let mut success_count = 0;
    let mut skip_count = 0;
    let mut error_count = 0;

    for record in prompts_to_update {
        match update_prompt_record(&record) {
            Ok(Some(updated_record)) => {
                eprintln!(
                    "  ✓ Updated {} ({}/{})",
                    &record.id[..8],
                    record.tool,
                    &record.external_thread_id[..min(16, record.external_thread_id.len())]
                );
                updated_records.push(updated_record);
                success_count += 1;
            }
            Ok(None) => {
                skip_count += 1;
            }
            Err(e) => {
                eprintln!("  ✗ Failed {} ({}): {}", &record.id[..8], record.tool, e);
                log_error(
                    &e,
                    Some(serde_json::json!({
                        "operation": "sync_prompts_update",
                        "prompt_id": record.id,
                        "tool": record.tool,
                    })),
                );
                error_count += 1;
            }
        }
    }

    // Batch upsert updated records
    if !updated_records.is_empty() {
        eprintln!(
            "\nBatch upserting {} updated prompts...",
            updated_records.len()
        );
        db_lock.batch_upsert_prompts(&updated_records)?;
    }

    eprintln!(
        "Update complete: {} updated, {} skipped, {} failed",
        success_count, skip_count, error_count
    );

    // ===== PHASE 2: DISCOVERY =====
    eprintln!("\n=== Phase 2: Discovering conversations ===");

    // Call discovery trait methods for each tool
    let cursor_result = CursorPreset::discover_conversations(since_timestamp);
    let claude_result = ClaudePreset::discover_conversations(since_timestamp);
    let continue_result = ContinueCliPreset::discover_conversations(since_timestamp);

    let total_discovered = cursor_result.conversations.len()
        + claude_result.conversations.len()
        + continue_result.conversations.len();

    eprintln!("Discovered {} conversations:", total_discovered);
    eprintln!("  - Cursor: {}", cursor_result.conversations.len());
    eprintln!("  - Claude Code: {}", claude_result.conversations.len());
    eprintln!("  - Continue CLI: {}", continue_result.conversations.len());

    // Report discovery warnings
    let all_errors: Vec<String> = cursor_result
        .errors
        .iter()
        .chain(claude_result.errors.iter())
        .chain(continue_result.errors.iter())
        .cloned()
        .collect();

    if !all_errors.is_empty() {
        eprintln!("\nDiscovery warnings:");
        for error in &all_errors {
            eprintln!("  ⚠ {}", error);
        }
    }

    // Combine all discovered conversations
    let all_discovered: Vec<DiscoveredConversation> = cursor_result
        .conversations
        .into_iter()
        .chain(claude_result.conversations.into_iter())
        .chain(continue_result.conversations.into_iter())
        .collect();

    // ===== PHASE 3: IMPORT NEW CONVERSATIONS =====
    eprintln!("\n=== Phase 3: Importing new conversations ===");
    let import_stats = import_discovered_conversations(&mut db_lock, all_discovered)?;

    eprintln!("Import results:");
    eprintln!("  ✓ Imported: {}", import_stats.imported);
    eprintln!("  - Already exists: {}", import_stats.already_exists);
    eprintln!("  - Skipped: {}", import_stats.skipped);
    eprintln!("  ✗ Failed: {}", import_stats.failed);

    // ===== SUMMARY =====
    eprintln!(
        "\n✓ Sync complete: {} imported, {} updated, {} skipped, {} failed",
        import_stats.imported,
        success_count,
        skip_count,
        error_count + import_stats.failed
    );

    Ok(())
}

fn deduplicate_by_agent_id(prompts: &[PromptDbRecord]) -> Vec<PromptDbRecord> {
    let mut latest_by_agent: HashMap<String, PromptDbRecord> = HashMap::new();

    for record in prompts {
        let key = format!("{}:{}", record.tool, record.external_thread_id);

        // Keep the record with latest updated_at
        latest_by_agent
            .entry(key)
            .and_modify(|existing| {
                if record.updated_at > existing.updated_at {
                    *existing = record.clone();
                }
            })
            .or_insert_with(|| record.clone());
    }

    latest_by_agent.into_values().collect()
}

fn update_prompt_record(
    record: &PromptDbRecord,
) -> Result<Option<PromptDbRecord>, GitAiError> {
    // Use shared update_prompt_from_tool from prompt_updater module
    let result = update_prompt_from_tool(
        &record.tool,
        &record.external_thread_id,
        record.agent_metadata.as_ref(),
        &record.model,
    );

    match result {
        PromptUpdateResult::Updated(new_transcript, new_model) => {
            // Check if transcript actually changed
            if new_transcript == record.messages {
                return Ok(None); // No actual change
            }

            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            let mut updated_record = record.clone();
            updated_record.messages = new_transcript;
            updated_record.model = new_model;
            updated_record.updated_at = now;

            Ok(Some(updated_record))
        }
        PromptUpdateResult::Unchanged => Ok(None),
        PromptUpdateResult::Failed(e) => Err(e),
    }
}

// ====================================================================
// Import Phase - Discovering and importing new conversations
// ====================================================================

/// Statistics for import operation
#[derive(Debug, Default)]
pub struct ImportStats {
    pub imported: usize,       // Successfully imported new conversations
    pub already_exists: usize, // Skipped because already in database
    pub skipped: usize,        // Skipped due to empty/invalid transcripts
    pub failed: usize,         // Failed to fetch or parse
}

/// Import discovered conversations into database
fn import_discovered_conversations(
    db: &mut InternalDatabase,
    all_discovered: Vec<DiscoveredConversation>,
) -> Result<ImportStats, GitAiError> {
    let mut stats = ImportStats::default();

    if all_discovered.is_empty() {
        return Ok(stats);
    }

    // Process in chunks of 100 for memory management
    const CHUNK_SIZE: usize = 100;
    let mut records_to_import = Vec::new();

    for conversation in all_discovered {
        // Generate hash for deduplication
        let hash = generate_short_hash(&conversation.id, &conversation.tool);

        // Check if already exists in database
        match db.get_prompt(&hash) {
            Ok(Some(_)) => {
                stats.already_exists += 1;
                continue;
            }
            Ok(None) => {
                // Not in database, try to import
            }
            Err(e) => {
                eprintln!("  ⚠ Failed to check existence for {}: {}", &hash[..8], e);
                stats.failed += 1;
                continue;
            }
        }

        // Fetch and create record
        match fetch_and_create_record(&conversation) {
            Ok(Some(record)) => {
                records_to_import.push(record);

                // Batch upsert when chunk is full
                if records_to_import.len() >= CHUNK_SIZE {
                    match db.batch_upsert_prompts(&records_to_import) {
                        Ok(_) => stats.imported += records_to_import.len(),
                        Err(e) => {
                            eprintln!("  ✗ Batch upsert failed: {}", e);
                            stats.failed += records_to_import.len();
                        }
                    }
                    records_to_import.clear();
                }
            }
            Ok(None) => {
                stats.skipped += 1;
            }
            Err(e) => {
                eprintln!("  ⚠ Failed to import {}: {}", &conversation.id[..8], e);
                stats.failed += 1;
            }
        }
    }

    // Upsert remaining records
    if !records_to_import.is_empty() {
        match db.batch_upsert_prompts(&records_to_import) {
            Ok(_) => stats.imported += records_to_import.len(),
            Err(e) => {
                eprintln!("  ✗ Final batch upsert failed: {}", e);
                stats.failed += records_to_import.len();
            }
        }
    }

    Ok(stats)
}

/// Fetch full transcript and create PromptDbRecord
fn fetch_and_create_record(
    conv: &DiscoveredConversation,
) -> Result<Option<PromptDbRecord>, GitAiError> {
    // Fetch transcript based on tool
    let (transcript, model) = match conv.tool.as_str() {
        "cursor" => fetch_cursor_transcript(&conv.id)?,
        "claude" => fetch_claude_transcript(&conv.agent_metadata)?,
        "continue-cli" => fetch_continue_transcript(&conv.agent_metadata)?,
        _ => {
            return Err(GitAiError::Generic(format!("Unknown tool: {}", conv.tool)));
        }
    };

    // Skip empty transcripts
    if transcript.messages.is_empty() {
        return Ok(None);
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    // Generate ID hash
    let id = generate_short_hash(&conv.id, &conv.tool);

    // Create PromptDbRecord
    let record = PromptDbRecord {
        id,
        tool: conv.tool.clone(),
        external_thread_id: conv.id.clone(),
        model: model.or(conv.model.clone()).unwrap_or_else(|| "unknown".to_string()),
        messages: transcript,
        workdir: conv.workdir.clone(),
        commit_sha: None,
        agent_metadata: if conv.agent_metadata.is_empty() {
            None
        } else {
            Some(conv.agent_metadata.clone())
        },
        human_author: None,          // Not available during discovery
        total_additions: None,        // Not available during discovery
        total_deletions: None,        // Not available during discovery
        accepted_lines: None,         // Not available during discovery
        overridden_lines: None,       // Not available during discovery
        created_at: conv.created_at.unwrap_or(now),
        updated_at: conv.updated_at.unwrap_or(now),
    };

    Ok(Some(record))
}

/// Fetch Cursor transcript
fn fetch_cursor_transcript(
    conversation_id: &str,
) -> Result<(crate::authorship::transcript::AiTranscript, Option<String>), GitAiError> {
    let (transcript, model) = CursorPreset::fetch_latest_cursor_conversation(conversation_id)?
        .ok_or_else(|| GitAiError::Generic("No transcript data found".to_string()))?;
    Ok((transcript, Some(model)))
}

/// Fetch Claude transcript
fn fetch_claude_transcript(
    metadata: &HashMap<String, String>,
) -> Result<(crate::authorship::transcript::AiTranscript, Option<String>), GitAiError> {
    let transcript_path = metadata
        .get("transcript_path")
        .ok_or_else(|| GitAiError::Generic("No transcript_path in metadata".to_string()))?;
    ClaudePreset::transcript_and_model_from_claude_code_jsonl(transcript_path)
}

/// Fetch Continue CLI transcript
fn fetch_continue_transcript(
    metadata: &HashMap<String, String>,
) -> Result<(crate::authorship::transcript::AiTranscript, Option<String>), GitAiError> {
    let transcript_path = metadata
        .get("transcript_path")
        .ok_or_else(|| GitAiError::Generic("No transcript_path in metadata".to_string()))?;
    let transcript = ContinueCliPreset::transcript_from_continue_json(transcript_path)?;
    Ok((transcript, None)) // Continue doesn't store model in transcript
}
