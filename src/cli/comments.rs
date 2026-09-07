//! CLI presentation for shared durable diff-review comment storage.

use super::{args::CommentCommands, diff, CliError};
pub(super) use crate::review_comments::Comment;
use crate::review_comments::{self, NewComment};
use serde_json::json;
use std::path::Path;

pub(super) fn run(command: &CommentCommands, json_output: bool) -> Result<(), CliError> {
    let repo = match command {
        CommentCommands::List { repo, .. }
        | CommentCommands::Add { repo, .. }
        | CommentCommands::Delete { repo, .. }
        | CommentCommands::Consume { repo, .. } => repo.as_deref(),
    };
    let root = diff::repository_root(repo.unwrap_or_else(|| Path::new(".")))?;
    match command {
        CommentCommands::List { all, .. } => {
            let comments = review_comments::load(&root).map_err(store_error)?;
            let listed: Vec<_> = comments
                .into_iter()
                .filter(|comment| *all || comment.consumed_at.is_none())
                .collect();
            print_list(&root, &listed, json_output)
        }
        CommentCommands::Add {
            file,
            side,
            line,
            end_line,
            line_text,
            message,
            ..
        } => {
            let comment = review_comments::add(
                &root,
                NewComment {
                    id: uuid::Uuid::new_v4(),
                    file_path: file.clone(),
                    side: side.clone(),
                    start_line: *line,
                    end_line: end_line.unwrap_or(*line),
                    line_text: line_text.clone(),
                    message: message.clone(),
                },
            )
            .map_err(store_error)?;
            print_comment(&root, &comment, json_output)
        }
        CommentCommands::Delete { id, .. } => {
            let id = parse_id(id)?;
            review_comments::delete(&root, id).map_err(store_error)?;
            if json_output {
                println!("{}", json!({"ok": true, "id": id, "repo_root": root}));
            } else {
                println!("Deleted review comment {id}");
            }
            Ok(())
        }
        CommentCommands::Consume { ids, all, .. } => {
            if !*all && ids.is_empty() {
                return Err(CliError::Command(
                    "comments consume requires one or more IDs or --all".into(),
                ));
            }
            let ids = ids
                .iter()
                .map(|id| parse_id(id))
                .collect::<Result<Vec<_>, _>>()?;
            let count = review_comments::consume(&root, &ids, *all).map_err(store_error)?;
            if json_output {
                println!(
                    "{}",
                    json!({"ok": true, "consumed": count, "repo_root": root})
                );
            } else {
                println!("Marked {count} review comment(s) consumed");
            }
            Ok(())
        }
    }
}

pub(super) fn for_viewer(root: &Path) -> Result<Vec<Comment>, CliError> {
    review_comments::pending(root).map_err(store_error)
}

fn parse_id(value: &str) -> Result<uuid::Uuid, CliError> {
    uuid::Uuid::parse_str(value).map_err(|_| CliError::Command("comment ID must be a UUID".into()))
}

fn store_error(error: std::io::Error) -> CliError {
    CliError::Command(format!("review comments: {error}"))
}

fn print_list(root: &Path, comments: &[Comment], json_output: bool) -> Result<(), CliError> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "repo_root": root,
                "count": comments.len(),
                "comments": comments,
            }))
            .map_err(|error| CliError::Command(error.to_string()))?
        );
        return Ok(());
    }
    if comments.is_empty() {
        println!("No review comments. (repo: {})", root.display());
        return Ok(());
    }
    println!(
        "{} review comment(s) (repo: {})",
        comments.len(),
        root.display()
    );
    for comment in comments {
        println!(
            "- {}:{} [{}] {}",
            comment.file_path, comment.end_line, comment.side, comment.message
        );
    }
    Ok(())
}

fn print_comment(root: &Path, comment: &Comment, json_output: bool) -> Result<(), CliError> {
    if json_output {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({"repo_root": root, "comment": comment}))
                .map_err(|error| CliError::Command(error.to_string()))?
        );
    } else {
        println!(
            "Saved review comment {} at {}:{}",
            comment.id, comment.file_path, comment.end_line
        );
    }
    Ok(())
}
