use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;

use fleet_protocol::{
    GuardrailPolicy, MissionTask, TaskRole, TaskStatus,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum GuardrailError {
    #[error("Sub-agent nesting depth limit reached (depth {current_depth} >= max {max_depth})")]
    MaxDepthExceeded { current_depth: u32, max_depth: u32 },
    #[error("Active sub-agent concurrency limit reached ({active} >= max {max})")]
    MaxConcurrencyExceeded { active: u32, max: u32 },
    #[error("Total sub-agent budget for this mission reached ({total} >= max {max})")]
    MaxSubagentsExceeded { total: u32, max: u32 },
    #[error("Duplicate active sub-agent goal detected: {0}")]
    DuplicateGoal(String),
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum DagError {
    #[error("Task `{task_id}` depends on unknown task `{dependency_id}`")]
    MissingDependency {
        task_id: String,
        dependency_id: String,
    },
    #[error("Dependency cycle detected in mission DAG")]
    CycleDetected,
}

/// Validates whether a parent task is allowed to spawn another child agent under mission guardrails.
pub fn check_spawn_guardrails(
    policy: &GuardrailPolicy,
    parent: &MissionTask,
    mission_tasks: &[MissionTask],
    new_title: &str,
) -> Result<(), GuardrailError> {
    let next_depth = parent.depth.saturating_add(1);
    if next_depth > policy.max_depth {
        return Err(GuardrailError::MaxDepthExceeded {
            current_depth: next_depth,
            max_depth: policy.max_depth,
        });
    }

    let total_subagents = mission_tasks
        .iter()
        .filter(|task| task.parent_task_id.is_some())
        .count() as u32;
    if total_subagents >= policy.max_subagents {
        return Err(GuardrailError::MaxSubagentsExceeded {
            total: total_subagents,
            max: policy.max_subagents,
        });
    }

    let active_subagents = mission_tasks
        .iter()
        .filter(|task| {
            task.parent_task_id.is_some()
                && matches!(
                    task.status,
                    TaskStatus::Pending
                        | TaskStatus::Queued
                        | TaskStatus::Running
                        | TaskStatus::WaitingInput
                        | TaskStatus::WaitingApproval
                )
        })
        .count() as u32;
    if active_subagents >= policy.max_concurrency {
        return Err(GuardrailError::MaxConcurrencyExceeded {
            active: active_subagents,
            max: policy.max_concurrency,
        });
    }

    let normalized_title = new_title.trim().to_ascii_lowercase();
    if mission_tasks.iter().any(|task| {
        task.parent_task_id.as_deref() == Some(parent.id.as_str())
            && task.title.trim().to_ascii_lowercase() == normalized_title
            && !matches!(
                task.status,
                TaskStatus::Completed | TaskStatus::Failed | TaskStatus::Cancelled
            )
    }) {
        return Err(GuardrailError::DuplicateGoal(new_title.to_owned()));
    }

    Ok(())
}

/// Validates that every `depends_on` reference exists and the task graph is acyclic (Kahn's algorithm).
pub fn validate_dag_acyclic(tasks: &[MissionTask]) -> Result<(), DagError> {
    let known: HashSet<&str> = tasks.iter().map(|t| t.id.as_str()).collect();
    let mut indegree: HashMap<&str, usize> = HashMap::new();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();

    for task in tasks {
        indegree.entry(task.id.as_str()).or_insert(0);
        for dep in &task.depends_on {
            if !known.contains(dep.as_str()) {
                return Err(DagError::MissingDependency {
                    task_id: task.id.clone(),
                    dependency_id: dep.clone(),
                });
            }
            *indegree.entry(task.id.as_str()).or_insert(0) += 1;
            dependents
                .entry(dep.as_str())
                .or_default()
                .push(task.id.as_str());
        }
    }

    let mut queue: VecDeque<&str> = indegree
        .iter()
        .filter_map(|(&id, &deg)| (deg == 0).then_some(id))
        .collect();
    let mut visited = 0usize;

    while let Some(node) = queue.pop_front() {
        visited += 1;
        if let Some(children) = dependents.get(node) {
            for &child in children {
                if let Some(deg) = indegree.get_mut(child) {
                    *deg -= 1;
                    if *deg == 0 {
                        queue.push_back(child);
                    }
                }
            }
        }
    }

    if visited == tasks.len() {
        Ok(())
    } else {
        Err(DagError::CycleDetected)
    }
}

/// Returns IDs of `Pending` tasks whose dependencies have all reached `Completed`.
pub fn find_ready_dag_tasks(tasks: &[MissionTask]) -> Vec<String> {
    let status_map: HashMap<&str, &TaskStatus> =
        tasks.iter().map(|t| (t.id.as_str(), &t.status)).collect();
    tasks
        .iter()
        .filter(|task| task.status == TaskStatus::Pending)
        .filter(|task| {
            task.depends_on
                .iter()
                .all(|dep| status_map.get(dep.as_str()) == Some(&&TaskStatus::Completed))
        })
        .map(|task| task.id.clone())
        .collect()
}

/// Computes the topological stage index (0, 1, 2...) for each task in a mission DAG.
pub fn compute_dag_layers(tasks: &[MissionTask]) -> HashMap<String, u32> {
    let by_id: HashMap<&str, &MissionTask> = tasks.iter().map(|t| (t.id.as_str(), t)).collect();
    let mut memo: HashMap<String, u32> = HashMap::new();

    fn resolve_layer(
        id: &str,
        by_id: &HashMap<&str, &MissionTask>,
        memo: &mut HashMap<String, u32>,
        visiting: &mut HashSet<String>,
    ) -> u32 {
        if let Some(&layer) = memo.get(id) {
            return layer;
        }
        if !visiting.insert(id.to_owned()) {
            return 0;
        }
        let layer = by_id
            .get(id)
            .map(|task| {
                task.depends_on
                    .iter()
                    .map(|dep| resolve_layer(dep, by_id, memo, visiting) + 1)
                    .max()
                    .unwrap_or(0)
            })
            .unwrap_or(0);
        visiting.remove(id);
        memo.insert(id.to_owned(), layer);
        layer
    }

    let mut visiting = HashSet::new();
    for task in tasks {
        resolve_layer(&task.id, &by_id, &mut memo, &mut visiting);
    }
    memo
}

/// Synthesizes a concise handoff section summarizing completed upstream dependencies.
pub fn build_upstream_handoff(task: &MissionTask, mission_tasks: &[MissionTask]) -> Option<String> {
    if task.depends_on.is_empty() {
        return None;
    }
    let mut blocks = Vec::new();
    for dep_id in &task.depends_on {
        if let Some(dep) = mission_tasks.iter().find(|t| &t.id == dep_id) {
            let mut block = format!(
                "- [{}] {} ({:?} · model={})",
                dep.id,
                dep.title,
                dep.status,
                dep.assigned_model_name.as_deref().unwrap_or("default")
            );
            if let Some(branch) = &dep.worktree_branch {
                block.push_str(&format!("\n  branch: {branch}"));
            }
            if let Some(diff) = &dep.diff_stat {
                block.push_str(&format!("\n  diff_stat: {diff}"));
            }
            if let Some(summary) = &dep.result_summary {
                let compact: String = summary.chars().take(600).collect();
                block.push_str(&format!("\n  summary: {compact}"));
            }
            blocks.push(block);
        }
    }
    if blocks.is_empty() {
        None
    } else {
        Some(format!("[Upstream Completed Tasks]\n{}", blocks.join("\n")))
    }
}

/// Formats the prompt header injected into a parent/child/planner task so it knows its Fleet
/// identity and how to invoke `termexo-fleet` MCP tools safely.
pub fn build_fleet_task_description(
    description: &str,
    mission_id: &str,
    task_id: &str,
    parent_token: &str,
    role: TaskRole,
    depth: u32,
    max_depth: u32,
    upstream_handoff: Option<&str>,
) -> String {
    let can_spawn = depth < max_depth;
    let mut header = format!(
        "[Termexo Fleet Context]\nmission_id: {mission_id}\ntask_id: {task_id}\nrole: {:?}\ndepth: {depth}/{max_depth}\n",
        role
    );
    if role == TaskRole::Planner {
        header.push_str(&format!(
            "parent_token: {parent_token}\n\
             You are the Planner for this mission. Inspect the repository first, optionally check online Termexo nodes via `fleet_list_nodes`, \
             then call `fleet_submit_plan(parent_token, steps)` with a clean DAG of executable tasks (each with `key`, `title`, `goal`, `depends_on_keys`, `difficulty`, `role`, `required_node_tags`, `isolate_worktree`). \
             Do NOT edit project source files directly unless required; let the downstream Worker/Reviewer tasks execute the plan.\n"
        ));
    } else if can_spawn {
        header.push_str(&format!(
            "parent_token: {parent_token}\n\
             You may delegate independent parallel subtasks via the `termexo-fleet` MCP tools \
             (`fleet_spawn_subagent`, `fleet_wait_subagents`, `fleet_list_subagents`, `fleet_message_subagent`, `fleet_cancel_subagent`, `fleet_list_nodes`) \
             passing the `parent_token` above.\n"
        ));
    } else {
        header.push_str(
            "Sub-agent spawning is disabled at max depth; complete this task directly.\n",
        );
    }
    if let Some(handoff) = upstream_handoff {
        header.push('\n');
        header.push_str(handoff);
        header.push('\n');
    }
    header.push_str("\n[Task Instructions]\n");
    header.push_str(description.trim());
    header
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeHandle {
    pub path: String,
    pub branch: String,
}

#[derive(Debug, Error)]
pub enum WorktreeError {
    #[error("Repository path does not exist: {0}")]
    MissingRepo(String),
    #[error("Git command failed: {0}")]
    Git(String),
}

/// Creates an isolated Git worktree sibling directory for parallel sub-agent edits.
pub fn create_isolated_worktree(
    repo_path: &Path,
    task_id: &str,
) -> Result<WorktreeHandle, WorktreeError> {
    if !repo_path.exists() {
        return Err(WorktreeError::MissingRepo(repo_path.display().to_string()));
    }
    let short_id: String = task_id
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .take(12)
        .collect();
    let suffix = if short_id.is_empty() {
        uuid::Uuid::new_v4().simple().to_string()[..8].to_owned()
    } else {
        short_id
    };
    let branch = format!("fleet/{suffix}");
    let worktree_dir = worktree_path_for(repo_path, &suffix);
    if let Some(parent) = worktree_dir.parent() {
        std::fs::create_dir_all(parent).map_err(|e| WorktreeError::Git(e.to_string()))?;
    }

    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .arg("worktree")
        .arg("add")
        .arg("-b")
        .arg(&branch)
        .arg(&worktree_dir)
        .output()
        .map_err(|e| WorktreeError::Git(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(WorktreeError::Git(stderr));
    }

    Ok(WorktreeHandle {
        path: worktree_dir.to_string_lossy().into_owned(),
        branch,
    })
}

/// Collects a compact `git diff --stat` (or `git status --short`) summary for a worktree.
pub fn collect_worktree_diff_stat(worktree_path: &Path) -> Option<String> {
    if !worktree_path.exists() {
        return None;
    }
    let output = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["diff", "--stat", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let diff = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !diff.is_empty() {
        return Some(diff);
    }
    let status = Command::new("git")
        .arg("-C")
        .arg(worktree_path)
        .args(["status", "--short"])
        .output()
        .ok()?;
    let short = String::from_utf8_lossy(&status.stdout).trim().to_owned();
    (!short.is_empty()).then_some(short)
}

/// Removes a worktree created by Fleet when a child task is cancelled or cleaned up.
pub fn remove_isolated_worktree(repo_path: &Path, worktree_path: &Path) -> Result<(), WorktreeError> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_path)
        .args(["worktree", "remove", "--force"])
        .arg(worktree_path)
        .output()
        .map_err(|e| WorktreeError::Git(e.to_string()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(WorktreeError::Git(stderr));
    }
    Ok(())
}

fn worktree_path_for(repo_path: &Path, suffix: &str) -> PathBuf {
    let repo_name = repo_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("repo");
    let base = repo_path.parent().unwrap_or(repo_path);
    base.join(".termexo-fleet-worktrees")
        .join(format!("{repo_name}-{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fleet_protocol::{DifficultyTier, ExecutionMode};

    fn fake_task(
        id: &str,
        parent: Option<&str>,
        depth: u32,
        title: &str,
        status: TaskStatus,
        depends_on: Vec<&str>,
    ) -> MissionTask {
        MissionTask {
            id: id.into(),
            mission_id: "m1".into(),
            parent_task_id: parent.map(str::to_owned),
            depth,
            depends_on: depends_on.into_iter().map(str::to_owned).collect(),
            required_node_tags: vec![],
            role: TaskRole::Worker,
            execution_mode: ExecutionMode::Task,
            preferred_terminal_id: None,
            title: title.into(),
            description: String::new(),
            acceptance_criteria: String::new(),
            difficulty: DifficultyTier::L2,
            auto_difficulty: false,
            preferred_agent: None,
            exclude_model_family: None,
            assigned_node_id: None,
            assigned_slot_id: None,
            assigned_agent_type: None,
            assigned_model_name: None,
            assigned_account_profile_id: None,
            routing_reason: None,
            termexo_task_id: None,
            termexo_terminal_id: None,
            worktree_path: None,
            worktree_branch: None,
            status,
            retry_count: 0,
            max_retries: 2,
            result_summary: None,
            diff_stat: None,
            last_error: None,
            parent_token: format!("tok-{id}"),
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn enforces_depth_concurrency_total_and_duplicate_guardrails() {
        let policy = GuardrailPolicy {
            max_depth: 2,
            max_concurrency: 2,
            max_subagents: 3,
            isolate_worktree_by_default: true,
        };
        let root = fake_task("root", None, 0, "Root", TaskStatus::Running, vec![]);
        let child1 = fake_task("c1", Some("root"), 1, "Write API", TaskStatus::Running, vec![]);
        let child2 = fake_task("c2", Some("c1"), 2, "Leaf", TaskStatus::Completed, vec![]);

        assert_eq!(
            check_spawn_guardrails(
                &policy,
                &child2,
                &[root.clone(), child1.clone(), child2.clone()],
                "Too deep"
            ),
            Err(GuardrailError::MaxDepthExceeded {
                current_depth: 3,
                max_depth: 2
            })
        );

        assert!(matches!(
            check_spawn_guardrails(&policy, &root, &[root.clone(), child1.clone()], "write api"),
            Err(GuardrailError::DuplicateGoal(_))
        ));

        let child_b = fake_task("c3", Some("root"), 1, "Write UI", TaskStatus::Running, vec![]);
        assert_eq!(
            check_spawn_guardrails(&policy, &root, &[root.clone(), child1.clone(), child_b], "Third"),
            Err(GuardrailError::MaxConcurrencyExceeded { active: 2, max: 2 })
        );

        let done1 = fake_task("d1", Some("root"), 1, "Done 1", TaskStatus::Completed, vec![]);
        let done2 = fake_task("d2", Some("root"), 1, "Done 2", TaskStatus::Completed, vec![]);
        let done3 = fake_task("d3", Some("root"), 1, "Done 3", TaskStatus::Completed, vec![]);
        assert_eq!(
            check_spawn_guardrails(&policy, &root, &[root.clone(), done1, done2, done3], "Fourth"),
            Err(GuardrailError::MaxSubagentsExceeded { total: 3, max: 3 })
        );
    }

    #[test]
    fn validates_dag_cycles_layers_ready_tasks_and_upstream_handoff() {
        let mut t1 = fake_task("t1", None, 0, "Implement", TaskStatus::Completed, vec![]);
        t1.result_summary = Some("Implemented OAuth2 flow".into());
        t1.diff_stat = Some("3 files changed".into());
        let t2 = fake_task("t2", None, 0, "Test", TaskStatus::Pending, vec!["t1"]);
        let t3 = fake_task("t3", None, 0, "Review", TaskStatus::Pending, vec!["t1", "t2"]);

        let dag = vec![t1.clone(), t2.clone(), t3.clone()];
        assert!(validate_dag_acyclic(&dag).is_ok());
        assert_eq!(find_ready_dag_tasks(&dag), vec!["t2".to_owned()]);

        let layers = compute_dag_layers(&dag);
        assert_eq!(layers.get("t1"), Some(&0));
        assert_eq!(layers.get("t2"), Some(&1));
        assert_eq!(layers.get("t3"), Some(&2));

        let handoff = build_upstream_handoff(&t2, &dag).unwrap();
        assert!(handoff.contains("Implemented OAuth2 flow"));
        assert!(handoff.contains("3 files changed"));

        // Cycle detection.
        let c1 = fake_task("a", None, 0, "A", TaskStatus::Pending, vec!["b"]);
        let c2 = fake_task("b", None, 0, "B", TaskStatus::Pending, vec!["a"]);
        assert_eq!(validate_dag_acyclic(&[c1, c2]), Err(DagError::CycleDetected));
    }

    #[test]
    fn injects_planner_and_worker_instructions_by_role_and_depth() {
        let planner = build_fleet_task_description(
            "Plan OAuth",
            "m1",
            "p1",
            "tok",
            TaskRole::Planner,
            0,
            2,
            None,
        );
        assert!(planner.contains("fleet_submit_plan"));
        assert!(planner.contains("fleet_list_nodes"));

        let leaf = build_fleet_task_description(
            "Leaf step",
            "m1",
            "t2",
            "tok",
            TaskRole::Worker,
            2,
            2,
            Some("[Upstream Completed Tasks]\n- done"),
        );
        assert!(!leaf.contains("parent_token"));
        assert!(leaf.contains("[Upstream Completed Tasks]"));
    }
}
