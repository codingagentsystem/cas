//! MCP Tools Service for CAS
//!
//! This module exposes consolidated meta-tools:
//! - cas_memory: All memory/entry operations
//! - cas_task: All task and dependency operations
//! - cas_rule: All rule operations
//! - cas_skill: All skill operations
//! - cas_coordination: Agent, factory, and worktree operations (merged)
//! - cas_search: Search, context, and entity operations
//! - cas_system: Diagnostics, stats, and maintenance
//! - cas_verification: Task quality gates
//! - cas_team: Team collaboration
//! - cas_pattern: Personal patterns
//! - cas_spec: Specifications
//! - mcp_search: Search tools across connected upstream MCP servers
//! - mcp_execute: Execute tool calls across connected upstream MCP servers

use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorCode};
#[cfg(feature = "mcp-proxy")]
use rmcp::model::Content;
use rmcp::{ErrorData as McpError, tool, tool_router};

use crate::mcp::server::CasCore;

mod imports;

// Re-export types from cas-mcp for MCP tool parameters
pub use cas_mcp::{
    AgentRequest, CoordinationRequest, ExecuteRequest, FactoryRequest, MemoryRequest,
    PatternRequest, RuleRequest, SearchContextRequest, SkillRequest, SpecRequest, SystemRequest,
    TaskRequest, TeamRequest, VerificationRequest,
};

// ============================================================================
// Git Blame Helper Types
// ============================================================================

/// A single line from git blame output
pub(super) struct GitBlameLine {
    pub(super) commit_hash: String,
    pub(super) line_number: usize,
    pub(super) author: String,
    pub(super) content: String,
}

/// Parse git blame porcelain format
pub(super) fn parse_git_blame_porcelain(content: &str) -> Vec<GitBlameLine> {
    let mut lines_iter = content.lines().peekable();
    let mut results = Vec::new();
    let mut line_number = 0usize;

    while let Some(header) = lines_iter.next() {
        // Header format: <hash> <orig-line> <final-line> [<num-lines>]
        let parts: Vec<&str> = header.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let commit_hash = parts[0].to_string();

        // Read metadata lines until we hit the content line (starts with \t)
        let mut author = String::new();

        while let Some(line) = lines_iter.peek() {
            if line.starts_with('\t') {
                break;
            }
            let meta_line = lines_iter.next().unwrap();

            if let Some(author_val) = meta_line.strip_prefix("author ") {
                author = author_val.to_string();
            }
        }

        // Read content line (prefixed with tab)
        if let Some(content_line) = lines_iter.next() {
            line_number += 1;
            let content = content_line.strip_prefix('\t').unwrap_or(content_line);

            results.push(GitBlameLine {
                commit_hash,
                line_number,
                author,
                content: content.to_string(),
            });
        }
    }

    results
}

pub(super) fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let mut end = max_len.min(s.len());
        while end > 0 && !s.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &s[..end])
    }
}

/// Internal worktree request type used by handler methods.
/// The MCP-facing type is CoordinationRequest; this is used for internal dispatch.
#[derive(Debug)]
pub struct WorktreeRequest {
    pub action: String,
    pub id: Option<String>,
    pub task_id: Option<String>,
    pub all: Option<bool>,
    pub status: Option<String>,
    pub orphans: Option<bool>,
    pub dry_run: Option<bool>,
    pub force: Option<bool>,
}

// ============================================================================
// Tool Router Implementation
// ============================================================================

use rmcp::handler::server::router::tool::ToolRouter;

/// CAS MCP service with consolidated meta-tools
///
/// Provides action-based tools that consolidate related operations,
/// reducing MCP tool context overhead. Agent, factory, and worktree
/// tools are merged into a single `coordination` tool.
///
/// When a proxy engine is configured (via `.cas/proxy.toml`), two additional
/// tools (`mcp_search` and `mcp_execute`) are exposed for routing through
/// upstream MCP servers.
#[derive(Clone)]
pub struct CasService {
    pub inner: CasCore,
    /// MCP proxy engine for upstream server aggregation (optional).
    #[cfg(feature = "mcp-proxy")]
    pub proxy: Option<std::sync::Arc<cmcp_core::ProxyEngine>>,
    /// Tool router used internally by rmcp's #[tool_router] macro
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl CasService {
    pub fn new(
        inner: CasCore,
        #[cfg(feature = "mcp-proxy")] proxy: Option<std::sync::Arc<cmcp_core::ProxyEngine>>,
    ) -> Self {
        Self {
            inner,
            #[cfg(feature = "mcp-proxy")]
            proxy,
            tool_router: Self::tool_router(),
        }
    }

    #[allow(dead_code)]
    fn success(text: impl Into<String>) -> CallToolResult {
        CasCore::success(text)
    }

    fn error(code: ErrorCode, message: impl Into<String>) -> McpError {
        CasCore::error(code, message)
    }
}

#[tool_router]
impl CasService {
    // ========================================================================
    // cas_memory - All memory operations
    // ========================================================================

    #[tool(
        description = "Memory operations. Actions: remember (store new), get (by ID), list, update, delete, archive, unarchive, helpful, harmful, recent, set_tier (working/cold/archive), opinion_reinforce, opinion_weaken, opinion_contradict."
    )]
    pub async fn memory(
        &self,
        Parameters(req): Parameters<MemoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let is_mutating = matches!(
            req.action.as_str(),
            "remember"
                | "update"
                | "delete"
                | "archive"
                | "unarchive"
                | "helpful"
                | "harmful"
                | "mark_reviewed"
                | "set_tier"
                | "opinion_reinforce"
                | "opinion_weaken"
                | "opinion_contradict"
        );

        // Verification jail check
        self.inner
            .authorize_agent_action("memory", &action, is_mutating)?;

        let result = match req.action.as_str() {
            "remember" => self.memory_remember(req).await,
            "get" => self.memory_get(req).await,
            "list" => self.memory_list(req).await,
            "update" => self.memory_update(req).await,
            "delete" => self.memory_delete(req).await,
            "archive" => self.memory_archive(req).await,
            "unarchive" => self.memory_unarchive(req).await,
            "helpful" => self.memory_helpful(req).await,
            "harmful" => self.memory_harmful(req).await,
            "mark_reviewed" => self.memory_mark_reviewed(req).await,
            "recent" => self.memory_recent(req).await,
            "set_tier" => self.memory_set_tier(req).await,
            "opinion_reinforce" => self.memory_opinion_reinforce(req).await,
            "opinion_weaken" => self.memory_opinion_weaken(req).await,
            "opinion_contradict" => self.memory_opinion_contradict(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown memory action: {}. Valid: remember, get, list, update, delete, archive, unarchive, helpful, harmful, mark_reviewed, recent, set_tier, opinion_reinforce, opinion_weaken, opinion_contradict",
                    req.action
                ),
            )),
        };

        // Notify client of resource changes (Claude Code 2.1.0+)
        if is_mutating && result.is_ok() {
            self.inner.notify_resources_changed().await;
        }

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("memory", &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_task - All task operations
    // ========================================================================

    #[tool(
        description = "Task operations. Actions: create, show, update, start, close, reopen, delete, list, ready (actionable), blocked, notes (add progress), dep_add, dep_remove, dep_list, claim, release, transfer, available, mine. Prefer `start` for normal worker execution; use `claim` for manual lease control/recovery. IMPORTANT for 'close': verification must pass first. Workers should attempt close; if close returns verification-required guidance, follow the indicated verifier ownership workflow."
    )]
    pub async fn task(
        &self,
        Parameters(req): Parameters<TaskRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let is_mutating = matches!(
            req.action.as_str(),
            "create"
                | "update"
                | "start"
                | "close"
                | "reopen"
                | "delete"
                | "notes"
                | "dep_add"
                | "dep_remove"
                | "claim"
                | "release"
                | "transfer"
        );

        // Verification jail check
        self.inner
            .authorize_agent_action("task", &action, is_mutating)?;

        let result = match req.action.as_str() {
            "create" => self.task_create(req).await,
            "show" => self.task_show(req).await,
            "update" => self.task_update(req).await,
            "start" => self.task_start(req).await,
            "close" => self.task_close(req).await,
            "reopen" => self.task_reopen(req).await,
            "delete" => self.task_delete(req).await,
            "list" => self.task_list(req).await,
            "ready" => self.task_ready(req).await,
            "blocked" => self.task_blocked(req).await,
            "notes" => self.task_notes(req).await,
            "dep_add" => self.task_dep_add(req).await,
            "dep_remove" => self.task_dep_remove(req).await,
            "dep_list" => self.task_dep_list(req).await,
            "claim" => self.task_claim(req).await,
            "release" => self.task_release(req).await,
            "transfer" => self.task_transfer(req).await,
            "available" => self.task_available(req).await,
            "mine" => self.task_mine(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown task action: {}. Valid: create, show, update, start, close, reopen, delete, list, ready, blocked, notes, dep_add, dep_remove, dep_list, claim, release, transfer, available, mine",
                    req.action
                ),
            )),
        };

        // Notify client of resource changes (Claude Code 2.1.0+)
        if is_mutating && result.is_ok() {
            self.inner.notify_resources_changed().await;
        }

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("task", &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_rule - All rule operations
    // ========================================================================

    #[tool(
        description = "Rule operations. Actions: create, show, update, delete, list (proven only), list_all, helpful (promotes to proven), harmful, sync (to .claude/rules/), check_similar (find similar existing rules)."
    )]
    pub async fn rule(
        &self,
        Parameters(req): Parameters<RuleRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let is_mutating = matches!(
            req.action.as_str(),
            "create" | "update" | "delete" | "helpful" | "harmful" | "sync"
        );

        // Verification jail check
        self.inner
            .authorize_agent_action("rule", &action, is_mutating)?;

        let result = match req.action.as_str() {
            "create" => self.rule_create(req).await,
            "show" => self.rule_show(req).await,
            "update" => self.rule_update(req).await,
            "delete" => self.rule_delete(req).await,
            "list" => self.rule_list(req).await,
            "list_all" => self.rule_list_all(req).await,
            "helpful" => self.rule_helpful(req).await,
            "harmful" => self.rule_harmful(req).await,
            "sync" => self.rule_sync(req).await,
            "check_similar" => self.rule_check_similar(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown rule action: {}. Valid: create, show, update, delete, list, list_all, helpful, harmful, sync, check_similar",
                    req.action
                ),
            )),
        };

        // Notify client of resource changes (Claude Code 2.1.0+)
        if is_mutating && result.is_ok() {
            self.inner.notify_resources_changed().await;
        }

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("rule", &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_skill - All skill operations
    // ========================================================================

    #[tool(
        description = "Skill operations. Actions: create, show, update, delete, list (enabled), list_all, enable, disable, sync (to .claude/skills/), use (record usage)."
    )]
    pub async fn skill(
        &self,
        Parameters(req): Parameters<SkillRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let is_mutating = matches!(
            req.action.as_str(),
            "create" | "update" | "delete" | "enable" | "disable" | "sync" | "use"
        );

        // Verification jail check
        self.inner
            .authorize_agent_action("skill", &action, is_mutating)?;

        let result = match req.action.as_str() {
            "create" => self.skill_create(req).await,
            "show" => self.skill_show(req).await,
            "update" => self.skill_update(req).await,
            "delete" => self.skill_delete(req).await,
            "list" => self.skill_list(req).await,
            "list_all" => self.skill_list_all(req).await,
            "enable" => self.skill_enable(req).await,
            "disable" => self.skill_disable(req).await,
            "sync" => self.skill_sync(req).await,
            "use" => self.skill_use(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown skill action: {}. Valid: create, show, update, delete, list, list_all, enable, disable, sync, use",
                    req.action
                ),
            )),
        };

        // Notify client of resource changes (Claude Code 2.1.0+)
        if is_mutating && result.is_ok() {
            self.inner.notify_resources_changed().await;
        }

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("skill", &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_coordination - Agent, factory, and worktree operations (merged)
    // ========================================================================

    #[tool(
        description = "Coordination operations combining agent, factory, and worktree management. Agent actions: register, unregister, whoami, heartbeat, agent_list, agent_cleanup, session_start, session_end, loop_start, loop_cancel, loop_status, lease_history, queue_notify, queue_poll, queue_peek, queue_ack, message. Factory actions: spawn_workers, shutdown_workers, worker_status, worker_activity, clear_context, my_context, sync_all_workers, gc_report, gc_cleanup, remind, remind_list, remind_cancel. Worktree actions: worktree_create, worktree_list, worktree_show, worktree_cleanup, worktree_merge, worktree_status. Only available in factory mode. For shutdown_workers, supervisor should verify worktree cleanliness/policy before issuing shutdown."
    )]
    pub async fn coordination(
        &self,
        Parameters(req): Parameters<CoordinationRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();

        let result = match action.as_str() {
            // ---- Agent domain ----
            "register" | "unregister" | "whoami" | "heartbeat" | "session_start"
            | "session_end" | "loop_start" | "loop_cancel" | "loop_status" | "lease_history"
            | "queue_notify" | "queue_poll" | "queue_peek" | "queue_ack" | "message" => {
                let agent_req = req.to_agent_request(&action);
                match action.as_str() {
                    "register" => self.agent_register(agent_req).await,
                    "unregister" => self.agent_unregister(agent_req).await,
                    "whoami" => self.agent_whoami(agent_req).await,
                    "heartbeat" => self.agent_heartbeat(agent_req).await,
                    "session_start" => self.agent_session_start(agent_req).await,
                    "session_end" => self.agent_session_end(agent_req).await,
                    "loop_start" => self.loop_start(agent_req).await,
                    "loop_cancel" => self.loop_cancel(agent_req).await,
                    "loop_status" => self.loop_status(agent_req).await,
                    "lease_history" => self.lease_history(agent_req).await,
                    "queue_notify" => self.queue_notify(agent_req).await,
                    "queue_poll" => self.queue_poll(agent_req).await,
                    "queue_peek" => self.queue_peek(agent_req).await,
                    "queue_ack" => self.queue_ack(agent_req).await,
                    "message" => self.message_send(agent_req).await,
                    _ => unreachable!(),
                }
            }
            // agent_list and agent_cleanup: prefixed to avoid collision with worktree
            "agent_list" => {
                let agent_req = req.to_agent_request("list");
                self.agent_list(agent_req).await
            }
            "agent_cleanup" => {
                let agent_req = req.to_agent_request("cleanup");
                self.agent_cleanup(agent_req).await
            }

            // ---- Factory domain ----
            "spawn_workers" | "shutdown_workers" | "worker_status" | "worker_activity"
            | "clear_context" | "my_context" | "sync_all_workers" | "gc_report" | "gc_cleanup"
            | "remind" | "remind_list" | "remind_cancel" => {
                let factory_req = req.to_factory_request();
                match action.as_str() {
                    "spawn_workers" => self.factory_spawn_workers(factory_req).await,
                    "shutdown_workers" => self.factory_shutdown_workers(factory_req).await,
                    "worker_status" => self.factory_worker_status(factory_req).await,
                    "clear_context" => self.factory_clear_context(factory_req).await,
                    "my_context" => self.factory_my_context(factory_req).await,
                    "worker_activity" => self.factory_worker_activity(factory_req).await,
                    "sync_all_workers" => self.factory_sync_all_workers(factory_req).await,
                    "gc_report" => self.factory_gc_report(factory_req).await,
                    "gc_cleanup" => self.factory_gc_cleanup(factory_req).await,
                    "remind" => self.factory_remind(factory_req).await,
                    "remind_list" => self.factory_remind_list(factory_req).await,
                    "remind_cancel" => self.factory_remind_cancel(factory_req).await,
                    _ => unreachable!(),
                }
            }

            // ---- Worktree domain (prefixed with worktree_) ----
            "worktree_create" | "worktree_list" | "worktree_show" | "worktree_cleanup"
            | "worktree_merge" | "worktree_status" => {
                let wt_action = action.strip_prefix("worktree_").unwrap();

                // Check if worktrees are enabled (status action always allowed)
                if wt_action != "status" {
                    let config =
                        crate::config::Config::load(&self.inner.cas_root).map_err(|e| {
                            Self::error(
                                ErrorCode::INTERNAL_ERROR,
                                format!("Failed to load config: {e}"),
                            )
                        })?;
                    if !config.worktrees_enabled() {
                        return Ok(Self::success(
                            "Worktrees are experimental and disabled by default.\n\n\
                            To enable, add to .cas/config.toml:\n\n\
                              worktrees:\n\
                                enabled: true\n\n\
                            Use `coordination action=worktree_status` to see current configuration.",
                        ));
                    }
                }

                let wt_req = WorktreeRequest {
                    action: wt_action.to_string(),
                    id: req.id,
                    task_id: req.task_id,
                    all: req.all,
                    status: req.status,
                    orphans: req.orphans,
                    dry_run: req.dry_run,
                    force: req.force,
                };
                match wt_action {
                    "create" => self.worktree_create(wt_req).await,
                    "list" => self.worktree_list(wt_req).await,
                    "show" => self.worktree_show(wt_req).await,
                    "cleanup" => self.worktree_cleanup(wt_req).await,
                    "merge" => self.worktree_merge(wt_req).await,
                    "status" => self.worktree_status(wt_req).await,
                    _ => unreachable!(),
                }
            }

            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown coordination action: '{action}'. Valid actions:\n\
                     Agent: register, unregister, whoami, heartbeat, agent_list, agent_cleanup, session_start, session_end, loop_start, loop_cancel, loop_status, lease_history, queue_notify, queue_poll, queue_peek, queue_ack, message\n\
                     Factory: spawn_workers, shutdown_workers, worker_status, worker_activity, clear_context, my_context, sync_all_workers, gc_report, gc_cleanup, remind, remind_list, remind_cancel\n\
                     Worktree: worktree_create, worktree_list, worktree_show, worktree_cleanup, worktree_merge, worktree_status"
                ),
            )),
        };

        // Track with domain-specific tool name for backwards-compatible telemetry
        let domain = if action.starts_with("worktree_") {
            "worktree"
        } else if matches!(
            action.as_str(),
            "spawn_workers"
                | "shutdown_workers"
                | "worker_status"
                | "worker_activity"
                | "clear_context"
                | "my_context"
                | "sync_all_workers"
                | "gc_report"
                | "gc_cleanup"
                | "remind"
                | "remind_list"
                | "remind_cancel"
        ) {
            "factory"
        } else {
            "agent"
        };
        crate::telemetry::track_mcp_tool(domain, &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_search - Search, context, and entity operations
    // ========================================================================

    #[tool(
        description = "Search and context operations. Actions: search (BM25 full-text), context (session context), context_for_subagent, observe (record observation), entity_list, entity_show, entity_extract, code_search (search code symbols), code_show (show symbol details), grep, blame."
    )]
    pub async fn search(
        &self,
        Parameters(req): Parameters<SearchContextRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let result = match req.action.as_str() {
            "search" => self.search_impl(req).await,
            "context" => self.context_impl(req).await,
            "context_for_subagent" => self.context_for_subagent_impl(req).await,
            "observe" => self.observe_impl(req).await,
            "entity_list" => self.entity_list_impl(req).await,
            "entity_show" => self.entity_show_impl(req).await,
            "entity_extract" => self.entity_extract_impl(req).await,
            "code_search" => self.code_search_impl(req).await,
            "code_show" => self.code_show_impl(req).await,
            "grep" => self.grep_impl(req).await,
            "blame" => self.blame_impl(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown search action: {}. Valid: search, context, context_for_subagent, observe, entity_list, entity_show, entity_extract, code_search, code_show, grep, blame",
                    req.action
                ),
            )),
        };

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("search", &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_system - System and maintenance operations
    // ========================================================================

    #[tool(
        description = "System operations. Actions: version (CAS version info), doctor (diagnostics), stats, info (system info), reindex (BM25 index), maintenance_run, maintenance_status, config_docs (full config reference), config_search (search configs by query), report_cas_bug (submit CAS bug to GitHub - ANONYMIZE DATA: remove paths, credentials, proprietary code before submitting), proxy_add (add upstream MCP server), proxy_remove (remove server), proxy_list (list servers)."
    )]
    pub async fn system(
        &self,
        Parameters(req): Parameters<SystemRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let result = match req.action.as_str() {
            "version" => self.system_version().await,
            "doctor" => self.system_doctor(req).await,
            "stats" => self.system_stats(req).await,
            "info" => self.system_info(req).await,
            "reindex" => self.system_reindex(req).await,
            "maintenance_run" => self.system_maintenance_run(req).await,
            "maintenance_status" => self.system_maintenance_status(req).await,
            "config_docs" => self.system_config_docs().await,
            "config_search" => self.system_config_search(req).await,
            "report_cas_bug" => self.system_report_cas_bug(req).await,
            #[cfg(feature = "mcp-proxy")]
            "proxy_add" => self.system_proxy_add(req).await,
            #[cfg(feature = "mcp-proxy")]
            "proxy_remove" => self.system_proxy_remove(req).await,
            #[cfg(feature = "mcp-proxy")]
            "proxy_list" => self.system_proxy_list(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown system action: {}. Valid: version, doctor, stats, info, reindex, maintenance_run, maintenance_status, config_docs, config_search, report_cas_bug{}",
                    req.action,
                    if cfg!(feature = "mcp-proxy") { ", proxy_add, proxy_remove, proxy_list" } else { "" }
                ),
            )),
        };

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("system", &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_verification - Verification operations (task quality gates)
    // ========================================================================

    #[tool(
        description = "Verification operations (task quality gates). Actions: add (record verification result), show (verification details), list (verifications for task), latest (most recent for task)."
    )]
    pub async fn verification(
        &self,
        Parameters(req): Parameters<VerificationRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let result = match req.action.as_str() {
            "add" => self.verification_add(req).await,
            "show" => self.verification_show(req).await,
            "list" => self.verification_list(req).await,
            "latest" => self.verification_latest(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown verification action: {}. Valid: add, show, list, latest",
                    req.action
                ),
            )),
        };

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("verification", &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_team - Team operations for multi-user collaboration
    // ========================================================================

    #[tool(
        description = "Team operations. Actions: list (teams user belongs to), show (team details and stats), members (list team members with roles), sync (trigger team push + pull)."
    )]
    pub async fn team(
        &self,
        Parameters(req): Parameters<TeamRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let result = match req.action.as_str() {
            "list" => self.team_list(req).await,
            "show" => self.team_show(req).await,
            "members" => self.team_members(req).await,
            "sync" => self.team_sync(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown team action: {}. Valid: list, show, members, sync",
                    req.action
                ),
            )),
        };

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("team", &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_pattern - Personal patterns (cross-project conventions)
    // ========================================================================

    #[tool(
        description = "Personal pattern operations (cross-project conventions). Actions: create (new pattern), list (with filters), show (by ID), update (modify fields), archive (soft delete), adopt (from rule), helpful (increment), harmful (increment). Team actions (require team_id): team_suggestions (list), team_new_suggestions (pending only), team_create_suggestion, team_share (share personal pattern), team_adopt (adopt suggestion), team_dismiss, team_recommend, team_archive_suggestion, team_suggestion_analytics."
    )]
    pub async fn pattern(
        &self,
        Parameters(req): Parameters<PatternRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let is_mutating = matches!(
            req.action.as_str(),
            "create"
                | "update"
                | "archive"
                | "adopt"
                | "helpful"
                | "harmful"
                | "team_create_suggestion"
                | "team_share"
                | "team_adopt"
                | "team_dismiss"
                | "team_recommend"
                | "team_archive_suggestion"
        );

        // Verification jail check
        self.inner
            .authorize_agent_action("pattern", &action, is_mutating)?;

        let result = match req.action.as_str() {
            "create" => self.pattern_create(req).await,
            "list" => self.pattern_list(req).await,
            "show" => self.pattern_show(req).await,
            "update" => self.pattern_update(req).await,
            "archive" => self.pattern_archive(req).await,
            "adopt" => self.pattern_adopt(req).await,
            "helpful" => self.pattern_helpful(req).await,
            "harmful" => self.pattern_harmful(req).await,
            "team_suggestions" => self.team_suggestions(req).await,
            "team_new_suggestions" => self.team_new_suggestions(req).await,
            "team_create_suggestion" => self.team_create_suggestion(req).await,
            "team_share" => self.team_share(req).await,
            "team_adopt" => self.team_adopt_suggestion(req).await,
            "team_dismiss" => self.team_dismiss_suggestion(req).await,
            "team_recommend" => self.team_recommend_suggestion(req).await,
            "team_archive_suggestion" => self.team_archive_suggestion(req).await,
            "team_suggestion_analytics" => self.team_suggestion_analytics(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown pattern action: {}. Valid: create, list, show, update, archive, adopt, helpful, harmful, team_suggestions, team_new_suggestions, team_create_suggestion, team_share, team_adopt, team_dismiss, team_recommend, team_archive_suggestion, team_suggestion_analytics",
                    req.action
                ),
            )),
        };

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("pattern", &action, result.is_ok());

        result
    }

    // ========================================================================
    // cas_spec - All spec operations
    // ========================================================================

    #[tool(
        description = "Spec operations. Actions: create, show, update, delete, list, approve, reject, supersede, link, unlink, sync."
    )]
    pub async fn spec(
        &self,
        Parameters(req): Parameters<SpecRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let is_mutating = matches!(
            req.action.as_str(),
            "create"
                | "update"
                | "delete"
                | "approve"
                | "reject"
                | "supersede"
                | "link"
                | "unlink"
                | "sync"
        );

        // Verification jail check
        self.inner
            .authorize_agent_action("spec", &action, is_mutating)?;

        let result = match req.action.as_str() {
            "create" => self.spec_create(req).await,
            "show" => self.spec_show(req).await,
            "update" => self.spec_update(req).await,
            "delete" => self.spec_delete(req).await,
            "list" => self.spec_list(req).await,
            "approve" => self.spec_approve(req).await,
            "reject" => self.spec_reject(req).await,
            "supersede" => self.spec_supersede(req).await,
            "link" => self.spec_link(req).await,
            "unlink" => self.spec_unlink(req).await,
            "sync" => self.spec_sync(req).await,
            "get_for_task" => self.spec_get_for_task(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!(
                    "Unknown spec action: {}. Valid: create, show, update, delete, list, approve, reject, supersede, link, unlink, sync, get_for_task",
                    req.action
                ),
            )),
        };

        // Notify client of resource changes (Claude Code 2.1.0+)
        if is_mutating && result.is_ok() {
            self.inner.notify_resources_changed().await;
        }

        // Track MCP tool usage
        crate::telemetry::track_mcp_tool("spec", &action, result.is_ok());

        result
    }

    // ========================================================================
    // mcp_search - Search across all connected MCP servers
    // ========================================================================

    #[tool(
        description = "Search across all tools from all connected MCP servers. Write TypeScript code to filter the tool catalog. A typed `tools` array is available with { server, name, description, input_schema } fields."
    )]
    pub async fn mcp_search(
        &self,
        #[allow(unused_variables)] Parameters(req): Parameters<ExecuteRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "mcp-proxy")]
        {
            let proxy = self.proxy.as_ref().ok_or_else(|| {
                Self::error(
                    ErrorCode::INVALID_REQUEST,
                    "MCP proxy not configured. Add upstream servers to .cas/proxy.toml",
                )
            })?;

            match proxy.search(&req.code, req.max_length).await {
                Ok(value) => {
                    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
                    crate::telemetry::track_mcp_tool("mcp_proxy", "search", true);
                    Ok(Self::success(text))
                }
                Err(e) => {
                    crate::telemetry::track_mcp_tool("mcp_proxy", "search", false);
                    Err(Self::error(
                        ErrorCode::INTERNAL_ERROR,
                        format!("MCP search failed: {e}"),
                    ))
                }
            }
        }

        #[cfg(not(feature = "mcp-proxy"))]
        Err(Self::error(
            ErrorCode::INVALID_REQUEST,
            "MCP proxy requires mcp-proxy feature. Build with: cargo build --features mcp-proxy",
        ))
    }

    // ========================================================================
    // mcp_execute - Execute tool calls across connected MCP servers
    // ========================================================================

    #[tool(
        description = "Execute TypeScript code that calls tools across all connected MCP servers. Each server is a typed global object (e.g. `canva`, `figma`) where every tool is an async function with typed parameters: `await server.tool_name({ param: value })`. Chain calls sequentially or run them in parallel with Promise.all across different servers."
    )]
    pub async fn mcp_execute(
        &self,
        #[allow(unused_variables)] Parameters(req): Parameters<ExecuteRequest>,
    ) -> Result<CallToolResult, McpError> {
        #[cfg(feature = "mcp-proxy")]
        {
            let proxy = self.proxy.as_ref().ok_or_else(|| {
                Self::error(
                    ErrorCode::INVALID_REQUEST,
                    "MCP proxy not configured. Add upstream servers to .cas/proxy.toml",
                )
            })?;

            match proxy.execute(&req.code, req.max_length).await {
                Ok(result) => {
                    crate::telemetry::track_mcp_tool("mcp_proxy", "execute", true);
                    let mut content = vec![Content::text(result.text)];
                    for img in result.images {
                        content.push(Content::image(img.data, img.mime_type));
                    }
                    Ok(CallToolResult::success(content))
                }
                Err(e) => {
                    crate::telemetry::track_mcp_tool("mcp_proxy", "execute", false);
                    Err(Self::error(
                        ErrorCode::INTERNAL_ERROR,
                        format!("MCP execute failed: {e}"),
                    ))
                }
            }
        }

        #[cfg(not(feature = "mcp-proxy"))]
        Err(Self::error(
            ErrorCode::INVALID_REQUEST,
            "MCP proxy requires mcp-proxy feature. Build with: cargo build --features mcp-proxy",
        ))
    }
}

// ============================================================================
// Backwards-compatible wrapper methods for tests
// These are not exposed as MCP tools; use `coordination` tool instead.
// ============================================================================

impl CasService {
    /// Wrapper for factory operations (used by tests). Delegates to coordination.
    #[allow(dead_code)]
    pub async fn factory(
        &self,
        Parameters(req): Parameters<FactoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let action = req.action.clone();
        let result = match action.as_str() {
            "spawn_workers" => self.factory_spawn_workers(req).await,
            "shutdown_workers" => self.factory_shutdown_workers(req).await,
            "worker_status" => self.factory_worker_status(req).await,
            "clear_context" => self.factory_clear_context(req).await,
            "my_context" => self.factory_my_context(req).await,
            "worker_activity" => self.factory_worker_activity(req).await,
            "sync_all_workers" => self.factory_sync_all_workers(req).await,
            "gc_report" => self.factory_gc_report(req).await,
            "gc_cleanup" => self.factory_gc_cleanup(req).await,
            "remind" => self.factory_remind(req).await,
            "remind_list" => self.factory_remind_list(req).await,
            "remind_cancel" => self.factory_remind_cancel(req).await,
            _ => Err(Self::error(
                ErrorCode::INVALID_PARAMS,
                format!("Unknown factory action: {action}"),
            )),
        };
        crate::telemetry::track_mcp_tool("factory", &action, result.is_ok());
        result
    }
}

// ============================================================================
// Implementation methods - delegate to inner CasService
// ============================================================================

mod agent_search_system;
mod core;
mod factory_ops;
mod factory_remind;
mod pattern_ops;
mod server_handler;
mod spec_ops;
mod worktree_verification_team_ops;
