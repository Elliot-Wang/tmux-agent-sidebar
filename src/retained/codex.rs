//! Optional read-only lookup for Codex's shared-daemon clients, which have no
//! pane hook session ID. Only an exact, unique cwd + displayed-name match is
//! accepted. Never infer identity from recency or run `resume --last`.
use crate::tmux::{AgentType, SessionInfo};
use serde_json::Value;
use std::{collections::HashSet, path::PathBuf, process::Command};

#[derive(Default)]
pub(super) struct Index {
    next_scan: u64,
    rows: Vec<Value>,
}
impl Index {
    pub(super) fn enrich(&mut self, sessions: &mut [SessionInfo], now: u64) {
        if now >= self.next_scan {
            self.next_scan = now.saturating_add(5);
            let paths: HashSet<_> = sessions
                .iter()
                .flat_map(|s| &s.windows)
                .flat_map(|w| &w.panes)
                .filter(|p| p.agent == AgentType::Codex && p.session_id.is_none())
                .map(|p| p.path.as_str())
                .collect();
            self.rows = read_index(&paths).unwrap_or_default();
        }
        for p in sessions
            .iter_mut()
            .flat_map(|s| &mut s.windows)
            .flat_map(|w| &mut w.panes)
        {
            if p.agent == AgentType::Codex && p.session_id.is_none() {
                p.session_id = unique_match(&self.rows, &p.path, &p.prompt);
            }
        }
    }
}
fn read_index(paths: &HashSet<&str>) -> Option<Vec<Value>> {
    if paths.is_empty() {
        return Some(vec![]);
    }
    let home = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".codex")))?;
    let db = std::fs::read_dir(home)
        .ok()?
        .filter_map(Result::ok)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            let version = name
                .strip_prefix("state_")?
                .strip_suffix(".sqlite")?
                .parse::<u32>()
                .ok()?;
            Some((version, e.path()))
        })
        .max_by_key(|(v, _)| *v)?
        .1;
    let escaped = paths
        .iter()
        .map(|p| format!("'{}'", p.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    // `name` is the generated/renamed title in recent Codex versions. Older
    // databases may only have `title`; try that schema without mutating it.
    for title in ["coalesce(nullif(name,''),title)", "title"] {
        let sql = format!(
            "SELECT id,cwd,{title} AS name FROM threads WHERE archived=0 AND cwd IN ({escaped});"
        );
        let out = Command::new("sqlite3")
            .args(["-readonly", "-json"])
            .arg(&db)
            .arg(sql)
            .output()
            .ok()?;
        if out.status.success() {
            return serde_json::from_slice(&out.stdout).ok();
        }
    }
    None
}
fn unique_match(rows: &[Value], cwd: &str, prompt: &str) -> Option<String> {
    let title = prompt
        .rsplit_once(" | ")
        .map_or(prompt, |(title, _)| title)
        .trim();
    if title.is_empty() {
        return None;
    }
    let candidates: HashSet<&str> = rows
        .iter()
        .filter(|r| r["cwd"].as_str() == Some(cwd) && r["name"].as_str() == Some(title))
        .filter_map(|r| r["id"].as_str())
        .collect();
    if candidates.len() == 1 {
        candidates.into_iter().next().map(str::to_string)
    } else {
        None
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn identity_requires_unique_name_and_directory() {
        let rows = vec![
            json!({"id":"a","cwd":"/repo","name":"task"}),
            json!({"id":"b","cwd":"/other","name":"task"}),
        ];
        assert_eq!(
            unique_match(&rows, "/repo", "task | repo"),
            Some("a".into())
        );
        assert_eq!(unique_match(&rows, "/repo", "different | repo"), None);
        let mut duplicate = rows;
        duplicate.push(json!({"id":"c","cwd":"/repo","name":"task"}));
        assert_eq!(unique_match(&duplicate, "/repo", "task | repo"), None);
    }
}
