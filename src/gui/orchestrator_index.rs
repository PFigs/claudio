//! Pure parsing + filesystem walk for ~/.claude/orchestrator state.
//! No GPUI dependency on purpose -- everything here is unit-testable.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandoffStatus {
    None,
    Blocker,
    Question,
    Done,
}

impl HandoffStatus {
    /// Classify a handoff.md body. Looks at the *last* `## <kind>` heading.
    /// Empty/whitespace body -> None.
    pub fn classify(body: &str) -> Self {
        if body.trim().is_empty() {
            return Self::None;
        }
        let mut latest = Self::None;
        for line in body.lines() {
            let trimmed = line.trim().to_ascii_lowercase();
            if let Some(rest) = trimmed.strip_prefix("## ") {
                latest = match rest.trim() {
                    "blocker" => Self::Blocker,
                    "question" => Self::Question,
                    "done" | "completed" | "complete" => Self::Done,
                    _ => latest,
                };
            }
        }
        latest
    }
}

#[derive(Debug, Deserialize)]
struct Frontmatter {
    state: Option<String>,
    updated_at: Option<String>,
    branch: Option<String>,
    base_branch: Option<String>,
    worktree: Option<PathBuf>,
    pr: Option<String>,
}

#[derive(Debug, Clone)]
pub struct FeatureSummary {
    pub repo: String,
    pub feature_slug: String,
    pub status_path: PathBuf,
    pub state: String,
    pub updated_at: Option<String>,
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub worktree: Option<PathBuf>,
    pub pr: Option<String>,
    pub last_log_line: Option<String>,
    pub handoff: HandoffStatus,
}

impl FeatureSummary {
    pub fn is_completed(&self) -> bool {
        matches!(
            self.state.as_str(),
            "done" | "completed" | "complete" | "merged" | "shipped"
        )
    }
}

/// Parse a STATUS.md text body. The path is used to derive the repo name
/// and feature slug -- expected layout is .../<repo>/<feature_slug>/STATUS.md.
pub fn parse_status_text(text: &str, path: &Path) -> Result<FeatureSummary> {
    let stripped = strip_frontmatter(text)
        .ok_or_else(|| anyhow!("missing YAML frontmatter in {}", path.display()))?;

    let fm: Frontmatter = serde_yaml::from_str(stripped.frontmatter)
        .with_context(|| format!("parsing frontmatter of {}", path.display()))?;

    let last_log_line = stripped
        .body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .last()
        .map(|l| l.trim_start_matches("- ").trim().to_string());

    let (repo, feature_slug) = derive_repo_and_feature(path)
        .with_context(|| format!("deriving repo/feature from {}", path.display()))?;

    Ok(FeatureSummary {
        repo,
        feature_slug,
        status_path: path.to_path_buf(),
        state: fm.state.unwrap_or_else(|| "unknown".to_string()),
        updated_at: fm.updated_at,
        branch: fm.branch,
        base_branch: fm.base_branch,
        worktree: fm.worktree,
        pr: fm.pr,
        last_log_line,
        handoff: HandoffStatus::None,
    })
}

struct Stripped<'a> {
    frontmatter: &'a str,
    body: &'a str,
}

fn strip_frontmatter(text: &str) -> Option<Stripped<'_>> {
    let trimmed = text.trim_start_matches('\u{feff}');
    if !trimmed.starts_with("---") {
        return None;
    }
    let after_first = &trimmed[3..].trim_start_matches('\n');
    let end = after_first.find("\n---")?;
    let frontmatter = &after_first[..end];
    let body = after_first[end + 4..].trim_start_matches('\n');
    Some(Stripped { frontmatter, body })
}

/// Expects path like `<root>/<repo>/<feature_slug>/STATUS.md`.
fn derive_repo_and_feature(path: &Path) -> Result<(String, String)> {
    let mut comps: Vec<&std::ffi::OsStr> =
        path.components().map(|c| c.as_os_str()).collect();
    if comps.last().map(|s| s == &std::ffi::OsStr::new("STATUS.md")) != Some(true) {
        return Err(anyhow!("expected path to end in STATUS.md"));
    }
    comps.pop();
    let feature = comps
        .pop()
        .ok_or_else(|| anyhow!("missing feature dir"))?
        .to_string_lossy()
        .to_string();
    let repo = comps
        .pop()
        .ok_or_else(|| anyhow!("missing repo dir"))?
        .to_string_lossy()
        .to_string();
    Ok((repo, feature))
}

/// Walk `~/.claude/orchestrator/*/*/STATUS.md` and return parsed summaries,
/// sorted by `updated_at` descending. Failures on individual files are
/// logged via tracing and skipped, not propagated -- one bad file should
/// not blank the whole sidebar.
pub fn scan_all() -> Vec<FeatureSummary> {
    let home = match dirs::home_dir() {
        Some(h) => h,
        None => return Vec::new(),
    };
    let pattern = home.join(".claude/orchestrator/*/*/STATUS.md");
    let pattern_str = pattern.to_string_lossy().to_string();

    let mut summaries = Vec::new();
    let entries = match glob::glob(&pattern_str) {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!("orchestrator glob failed: {e}");
            return summaries;
        }
    };

    for entry in entries.flatten() {
        let text = match std::fs::read_to_string(&entry) {
            Ok(t) => t,
            Err(e) => {
                tracing::debug!("orchestrator: skip {}: {e}", entry.display());
                continue;
            }
        };
        let mut summary = match parse_status_text(&text, &entry) {
            Ok(s) => s,
            Err(e) => {
                tracing::debug!("orchestrator: parse fail {}: {e}", entry.display());
                continue;
            }
        };

        let handoff_path = entry
            .parent()
            .map(|p| p.join("handoff.md"))
            .unwrap_or_default();
        let handoff_text = std::fs::read_to_string(&handoff_path).unwrap_or_default();
        summary.handoff = HandoffStatus::classify(&handoff_text);

        summaries.push(summary);
    }

    summaries.sort_by(|a, b| {
        b.updated_at
            .as_deref()
            .unwrap_or("")
            .cmp(a.updated_at.as_deref().unwrap_or(""))
    });
    summaries
}
