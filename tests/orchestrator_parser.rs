use ok_claude::gui::orchestrator_index::{parse_status_text, HandoffStatus};
use std::path::PathBuf;

#[test]
fn parses_minimal_status() {
    let body = r#"---
state: in_progress
updated_at: 2026-05-11T13:00:00Z
branch: feat-x
worktree: /tmp/wt
---
- 2026-05-11T13:00:00Z orchestrator created feature dir
- 2026-05-11T13:05:00Z spawned via claudio
"#;

    let parsed = parse_status_text(
        body,
        "/home/me/.claude/orchestrator/repo/2026-05-11-feat-x/STATUS.md".as_ref(),
    )
    .expect("parses");
    assert_eq!(parsed.feature_slug, "2026-05-11-feat-x");
    assert_eq!(parsed.repo, "repo");
    assert_eq!(parsed.state, "in_progress");
    assert_eq!(parsed.branch.as_deref(), Some("feat-x"));
    assert_eq!(parsed.worktree, Some(PathBuf::from("/tmp/wt")));
    assert_eq!(
        parsed.last_log_line.as_deref(),
        Some("2026-05-11T13:05:00Z spawned via claudio")
    );
}

#[test]
fn missing_frontmatter_is_an_error() {
    let body = "no frontmatter here\n";
    assert!(parse_status_text(body, "/x/repo/feat/STATUS.md".as_ref()).is_err());
}

#[test]
fn classify_handoff_blocker() {
    let body = "## blocker\nThe build is broken because of X.\n";
    assert_eq!(HandoffStatus::classify(body), HandoffStatus::Blocker);
}

#[test]
fn classify_handoff_question() {
    let body = "## question\nShould I bump the dep?\n";
    assert_eq!(HandoffStatus::classify(body), HandoffStatus::Question);
}

#[test]
fn classify_handoff_empty() {
    assert_eq!(HandoffStatus::classify(""), HandoffStatus::None);
    assert_eq!(HandoffStatus::classify("   \n  "), HandoffStatus::None);
}

#[test]
fn classify_handoff_done() {
    let body = "## done\nMerged in #123.\n";
    assert_eq!(HandoffStatus::classify(body), HandoffStatus::Done);
}
