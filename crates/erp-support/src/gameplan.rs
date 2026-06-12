/// Task 4.2 — Gameplan Collaborative Thread Engine
///
/// Implements a graph-based discussion thread system using SurrealDB RELATE
/// statements to link Posts, Replies, and Reactions as first-class graph edges.
/// This is the Rust equivalent of the Frappe "Gameplan" collaborative app.
use serde::{Deserialize, Serialize};
use surrealdb::Surreal;
use surrealdb::engine::any::Any;
use chrono::{DateTime, Utc};
use uuid::Uuid;

// ── Domain types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub title: String,
    pub description: Option<String>,
    pub owner: String,
    pub created_at: DateTime<Utc>,
    pub is_archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Thread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub content: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    pub is_closed: bool,
    pub pinned: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    pub content: String,
    pub author: String,
    pub created_at: DateTime<Utc>,
    /// None = top-level comment on thread; Some = reply to another comment
    pub parent_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reaction {
    pub emoji: String,
    pub user: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadWithComments {
    pub thread: Thread,
    pub comments: Vec<CommentNode>,
    pub reaction_summary: Vec<ReactionCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommentNode {
    pub comment: Comment,
    pub replies: Vec<Comment>,
    pub reactions: Vec<ReactionCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReactionCount {
    pub emoji: String,
    pub count: u64,
    pub reacted_by_me: bool,
}

// ── Engine ────────────────────────────────────────────────────────────────────

pub struct GameplanEngine;

impl GameplanEngine {
    // ── Projects ──────────────────────────────────────────────────────────────

    pub async fn create_project(
        db: &Surreal<Any>,
        title: &str,
        description: Option<&str>,
        owner: &str,
    ) -> Result<Project, String> {
        let id = Uuid::new_v4().to_string();
        let project = Project {
            id: id.clone(),
            title: title.to_string(),
            description: description.map(|s| s.to_string()),
            owner: owner.to_string(),
            created_at: Utc::now(),
            is_archived: false,
        };

        db.query(
            "CREATE tabGameplanProject:$id CONTENT { \
                id: $id, title: $title, description: $description, \
                owner: $owner, created_at: $created_at, is_archived: false \
            };",
        )
        .bind(("id", id))
        .bind(("title", project.title.clone()))
        .bind(("description", project.description.clone()))
        .bind(("owner", project.owner.clone()))
        .bind(("created_at", project.created_at.to_rfc3339()))
        .await
        .map_err(|e| e.to_string())?;

        Ok(project)
    }

    // ── Threads ───────────────────────────────────────────────────────────────

    pub async fn create_thread(
        db: &Surreal<Any>,
        project_id: &str,
        title: &str,
        content: &str,
        author: &str,
    ) -> Result<Thread, String> {
        let id = Uuid::new_v4().to_string();
        let thread = Thread {
            id: id.clone(),
            project_id: project_id.to_string(),
            title: title.to_string(),
            content: content.to_string(),
            author: author.to_string(),
            created_at: Utc::now(),
            is_closed: false,
            pinned: false,
        };

        db.query(
            "CREATE tabGameplanThread:$id CONTENT { \
                id: $id, project_id: $project_id, title: $title, content: $content, \
                author: $author, created_at: $created_at, is_closed: false, pinned: false \
            };",
        )
        .bind(("id", id.clone()))
        .bind(("project_id", project_id.to_string()))
        .bind(("title", title.to_string()))
        .bind(("content", content.to_string()))
        .bind(("author", author.to_string()))
        .bind(("created_at", thread.created_at.to_rfc3339()))
        .await
        .map_err(|e| e.to_string())?;

        // Create a SurrealDB RELATE edge: project → HOSTS → thread
        db.query(
            "RELATE tabGameplanProject:$project_id->hosts->tabGameplanThread:$thread_id;",
        )
        .bind(("project_id", project_id.to_string()))
        .bind(("thread_id", id))
        .await
        .map_err(|e| e.to_string())?;

        Ok(thread)
    }

    pub async fn list_threads(
        db: &Surreal<Any>,
        project_id: &str,
    ) -> Result<Vec<Thread>, String> {
        let mut res = db
            .query(
                "SELECT * FROM tabGameplanThread WHERE project_id = $project_id \
                 ORDER BY pinned DESC, created_at DESC;",
            )
            .bind(("project_id", project_id.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        let threads: Vec<Thread> = rows
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();
        Ok(threads)
    }

    // ── Comments (tree structure via parent_id) ───────────────────────────────

    pub async fn post_comment(
        db: &Surreal<Any>,
        thread_id: &str,
        content: &str,
        author: &str,
        parent_comment_id: Option<&str>,
    ) -> Result<Comment, String> {
        let id = Uuid::new_v4().to_string();
        let comment = Comment {
            id: id.clone(),
            content: content.to_string(),
            author: author.to_string(),
            created_at: Utc::now(),
            parent_id: parent_comment_id.map(|s| s.to_string()),
        };

        db.query(
            "CREATE tabGameplanComment:$id CONTENT { \
                id: $id, thread_id: $thread_id, content: $content, \
                author: $author, created_at: $created_at, parent_id: $parent_id \
            };",
        )
        .bind(("id", id.clone()))
        .bind(("thread_id", thread_id.to_string()))
        .bind(("content", content.to_string()))
        .bind(("author", author.to_string()))
        .bind(("created_at", comment.created_at.to_rfc3339()))
        .bind(("parent_id", comment.parent_id.clone()))
        .await
        .map_err(|e| e.to_string())?;

        // Graph edge: thread → HAS_COMMENT → comment
        let relation_target = match &comment.parent_id {
            Some(parent) => format!(
                "RELATE tabGameplanComment:{}->replies_to->tabGameplanComment:{};",
                id, parent
            ),
            None => format!(
                "RELATE tabGameplanThread:{}->has_comment->tabGameplanComment:{};",
                thread_id, id
            ),
        };
        db.query(&relation_target)
            .await
            .map_err(|e| e.to_string())?;

        Ok(comment)
    }

    /// Returns a thread with all its top-level comments and their replies.
    pub async fn get_thread_with_comments(
        db: &Surreal<Any>,
        thread_id: &str,
    ) -> Result<ThreadWithComments, String> {
        // Fetch thread
        let mut res = db
            .query("SELECT * FROM tabGameplanThread:$id;")
            .bind(("id", thread_id.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        let thread_rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        let thread: Thread = thread_rows
            .into_iter()
            .next()
            .and_then(|v| serde_json::from_value(v).ok())
            .ok_or_else(|| format!("Thread {} not found", thread_id))?;

        // Fetch all comments for this thread
        let mut res2 = db
            .query(
                "SELECT * FROM tabGameplanComment WHERE thread_id = $thread_id \
                 ORDER BY created_at ASC;",
            )
            .bind(("thread_id", thread_id.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        let comment_rows: Vec<serde_json::Value> = res2.take(0).unwrap_or_default();
        let all_comments: Vec<Comment> = comment_rows
            .into_iter()
            .filter_map(|v| serde_json::from_value(v).ok())
            .collect();

        // Build tree: top-level comments with nested replies
        let top_level: Vec<Comment> = all_comments
            .iter()
            .filter(|c| c.parent_id.is_none())
            .cloned()
            .collect();

        let comment_nodes: Vec<CommentNode> = top_level
            .into_iter()
            .map(|comment| {
                let replies: Vec<Comment> = all_comments
                    .iter()
                    .filter(|c| c.parent_id.as_deref() == Some(&comment.id))
                    .cloned()
                    .collect();
                CommentNode {
                    comment,
                    replies,
                    reactions: Vec::new(), // populated separately if needed
                }
            })
            .collect();

        Ok(ThreadWithComments {
            thread,
            comments: comment_nodes,
            reaction_summary: Vec::new(),
        })
    }

    // ── Reactions ─────────────────────────────────────────────────────────────

    /// Toggles an emoji reaction on a comment (adds if absent, removes if present).
    pub async fn toggle_reaction(
        db: &Surreal<Any>,
        comment_id: &str,
        emoji: &str,
        user: &str,
    ) -> Result<(), String> {
        // Check if reaction already exists
        let mut res = db
            .query(
                "SELECT * FROM tabGameplanReaction \
                 WHERE comment_id = $comment_id AND emoji = $emoji AND user = $user \
                 LIMIT 1;",
            )
            .bind(("comment_id", comment_id.to_string()))
            .bind(("emoji", emoji.to_string()))
            .bind(("user", user.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        let existing: Vec<serde_json::Value> = res.take(0).unwrap_or_default();

        if existing.is_empty() {
            // Add reaction
            let id = Uuid::new_v4().to_string();
            db.query(
                "CREATE tabGameplanReaction:$id CONTENT { \
                    comment_id: $comment_id, emoji: $emoji, user: $user, created_at: $now \
                };",
            )
            .bind(("id", id))
            .bind(("comment_id", comment_id.to_string()))
            .bind(("emoji", emoji.to_string()))
            .bind(("user", user.to_string()))
            .bind(("now", Utc::now().to_rfc3339()))
            .await
            .map_err(|e| e.to_string())?;
        } else {
            // Remove reaction
            let reaction_id = existing[0]["id"].as_str().unwrap_or("").to_string();
            db.query("DELETE tabGameplanReaction:$id;")
                .bind(("id", reaction_id))
                .await
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    /// Returns grouped reaction counts for a comment.
    pub async fn get_reactions(
        db: &Surreal<Any>,
        comment_id: &str,
        current_user: &str,
    ) -> Result<Vec<ReactionCount>, String> {
        let mut res = db
            .query(
                "SELECT emoji, count() AS cnt, array::group(user) AS users \
                 FROM tabGameplanReaction \
                 WHERE comment_id = $comment_id \
                 GROUP BY emoji;",
            )
            .bind(("comment_id", comment_id.to_string()))
            .await
            .map_err(|e| e.to_string())?;

        let rows: Vec<serde_json::Value> = res.take(0).unwrap_or_default();
        let mut counts = Vec::new();

        for row in rows {
            let emoji = row["emoji"].as_str().unwrap_or("").to_string();
            let count = row["cnt"].as_u64().unwrap_or(0);
            let users: Vec<String> = row["users"]
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            let reacted_by_me = users.iter().any(|u| u == current_user);
            counts.push(ReactionCount { emoji, count, reacted_by_me });
        }

        Ok(counts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_creation_defaults() {
        let thread = Thread {
            id: "t1".to_string(),
            project_id: "p1".to_string(),
            title: "Q3 Planning".to_string(),
            content: "Let's plan Q3 objectives".to_string(),
            author: "admin@example.com".to_string(),
            created_at: Utc::now(),
            is_closed: false,
            pinned: false,
        };
        assert!(!thread.is_closed);
        assert!(!thread.pinned);
    }

    #[test]
    fn comment_tree_parent_linkage() {
        let top = Comment {
            id: "c1".to_string(),
            content: "Top-level comment".to_string(),
            author: "user1".to_string(),
            created_at: Utc::now(),
            parent_id: None,
        };
        let reply = Comment {
            id: "c2".to_string(),
            content: "A reply".to_string(),
            author: "user2".to_string(),
            created_at: Utc::now(),
            parent_id: Some("c1".to_string()),
        };
        assert!(top.parent_id.is_none());
        assert_eq!(reply.parent_id.as_deref(), Some("c1"));
    }
}
