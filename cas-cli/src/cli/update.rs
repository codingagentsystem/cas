//! Self-update command for CAS CLI
//!
//! Downloads and installs the latest version from GitHub releases,
//! and runs schema migrations for the local database.

use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use clap::Args;

use crate::builtins::sync_all_builtins_for_harness;
use crate::cli::Cli;
use crate::cli::factory_tooling;
use crate::cli::hook::{configure_claude_hooks, configure_codex_mcp_server, configure_mcp_server};
use crate::cli::init::{generate_cas_skill, update_claude_md};
use crate::cli::update::preview::{build_update_transaction, show_enhanced_dry_run};
use crate::migration::{check_migrations, run_migrations};
use crate::store::{open_rule_store, open_skill_store};
use crate::sync::{SkillSyncer, Syncer};
use crate::ui::components::Formatter;
use crate::ui::theme::ActiveTheme;

mod preview;

/// GitHub repository owner
const REPO_OWNER: &str = "codingagentsystem";

/// GitHub repository name
const REPO_NAME: &str = "cas";

/// Binary name in release assets
const BIN_NAME: &str = "cas";

#[derive(Args)]
pub struct UpdateArgs {
    /// Only check for updates without installing
    #[arg(long)]
    pub check: bool,

    /// Update to a specific version (e.g., "0.2.1")
    #[arg(long)]
    pub version: Option<String>,

    /// Skip confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Only run schema migrations (skip binary update)
    #[arg(long)]
    pub schema_only: bool,

    /// Only sync .claude/.codex files (agents, skills, rules, settings)
    #[arg(long)]
    pub sync: bool,

    /// Show what migrations would be applied without running them
    #[arg(long)]
    pub dry_run: bool,

    /// Keep backup files after successful update
    #[arg(long)]
    pub keep_backup: bool,
}

pub fn execute(args: &UpdateArgs, cli: &Cli, cas_root: Option<&Path>) -> anyhow::Result<()> {
    // Note: update command accepts Option<&Path> because it can run without an initialized CAS
    // (e.g., binary update only, or checking for updates before init)
    let current_version = env!("CARGO_PKG_VERSION");

    // Handle sync-only mode (just sync .claude/.codex files)
    if args.sync {
        let mut steps = UpdateStepTracker::new(1, !cli.json);
        return steps.run("Syncing .claude/.codex files", || {
            sync_claude_files(cli, cas_root)
        });
    }

    // Handle schema-only mode
    if args.schema_only || args.dry_run {
        let mut steps = UpdateStepTracker::new(1, !cli.json);
        return steps.run("Applying schema updates", || {
            run_schema_migrations(args, cli, cas_root)
        });
    }

    // Handle check mode (includes schema status)
    if args.check {
        return check_for_updates(current_version, cli, cas_root);
    }

    // Full update: binary + schema + sync files
    let mut steps = UpdateStepTracker::new(3, !cli.json);
    steps.run("Updating CAS binary", || {
        perform_update(args, current_version, cli)
    })?;
    if !cli.json {
        let mut out = io::stdout();
        let theme = ActiveTheme::default();
        let mut fmt = Formatter::stdout(&mut out, theme);
        fmt.newline()?;
    }

    steps.run("Applying schema updates", || {
        run_schema_migrations(args, cli, cas_root)
    })?;
    if !cli.json {
        let mut out = io::stdout();
        let theme = ActiveTheme::default();
        let mut fmt = Formatter::stdout(&mut out, theme);
        fmt.newline()?;
    }

    steps.run("Syncing .claude/.codex files", || {
        sync_claude_files(cli, cas_root)
    })?;

    if !cli.json {
        let mut out = io::stdout();
        let theme = ActiveTheme::default();
        let mut fmt = Formatter::stdout(&mut out, theme);
        fmt.newline()?;
        fmt.success("Update completed")?;
    }

    Ok(())
}

/// Sync rules, skills, and configuration to .claude/.codex directories
fn sync_claude_files(cli: &Cli, cas_root_param: Option<&Path>) -> anyhow::Result<()> {
    // cas_root is optional - if not provided and CAS is not initialized, nothing to sync
    let cas_root = match cas_root_param {
        Some(path) => path.to_path_buf(),
        None => {
            // Not initialized, nothing to sync
            return Ok(());
        }
    };

    let project_root = cas_root.parent().unwrap_or(&cas_root);
    let claude_dir = project_root.join(".claude");
    let codex_dir = project_root.join(".codex");
    let codex_enabled = codex_dir.exists();

    let theme = ActiveTheme::default();

    if !cli.json {
        let mut out = io::stdout();
        let mut fmt = Formatter::stdout(&mut out, theme.clone());
        fmt.subheading("Syncing .claude files")?;
    }

    // Track what was updated for JSON output
    let mut config_updated = Vec::new();
    let mut codex_config_updated = Vec::new();

    // Update configuration files
    // 1. Claude Code hooks (.claude/settings.json)
    match configure_claude_hooks(project_root, false) {
        Ok(true) => {
            config_updated.push("settings.json");
            if !cli.json {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.success("Updated .claude/settings.json")?;
            }
        }
        Ok(false) => {} // No changes needed
        Err(e) => {
            if !cli.json {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.warning(&format!("Could not update settings.json: {e}"))?;
            }
        }
    }

    // 2. MCP server configuration (.mcp.json)
    match configure_mcp_server(project_root) {
        Ok(true) => {
            config_updated.push(".mcp.json");
            if !cli.json {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.success("Updated .mcp.json")?;
            }
        }
        Ok(false) => {} // No changes needed
        Err(e) => {
            if !cli.json {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.warning(&format!("Could not update .mcp.json: {e}"))?;
            }
        }
    }

    // 3. CLAUDE.md directive
    match update_claude_md(project_root) {
        Ok(true) => {
            config_updated.push("CLAUDE.md");
            if !cli.json {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.success("Updated CLAUDE.md")?;
            }
        }
        Ok(false) => {} // No changes needed
        Err(e) => {
            if !cli.json {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.warning(&format!("Could not update CLAUDE.md: {e}"))?;
            }
        }
    }

    // 4. Main CAS skill (.claude/skills/cas/SKILL.md)
    match generate_cas_skill(project_root) {
        Ok(true) => {
            config_updated.push("skills/cas/SKILL.md");
            if !cli.json {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.success("Updated .claude/skills/cas/SKILL.md")?;
            }
        }
        Ok(false) => {} // No changes needed or user-customized
        Err(e) => {
            if !cli.json {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.warning(&format!("Could not update CAS skill: {e}"))?;
            }
        }
    }

    // Sync database rules
    let rule_store = open_rule_store(&cas_root)?;
    let rules = rule_store.list()?;
    let rule_syncer = Syncer::with_defaults(project_root);
    let rule_report = rule_syncer.sync_all(&rules)?;

    // Sync database skills (this may remove stale dirs)
    let skill_store = open_skill_store(&cas_root)?;
    let skills = skill_store.list(None)?;
    let skill_syncer = SkillSyncer::with_defaults(project_root);
    let skill_report = skill_syncer.sync_all(&skills)?;

    // Sync built-in agents, skills, and commands AFTER database sync
    // (so they don't get removed as "stale" by the skill syncer)
    let builtin_result =
        sync_all_builtins_for_harness(cas_mux::SupervisorCli::Claude, &claude_dir)?;

    // Sync factory tooling helper templates.
    let factory_tooling_result = match factory_tooling::setup_factory_tooling(project_root) {
        Ok(summary) => {
            if !cli.json && !summary.is_empty() {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.success(&format!("Factory tooling: {summary}"))?;
            }
            summary
        }
        Err(e) => {
            if !cli.json {
                let mut out = io::stdout();
                let mut fmt = Formatter::stdout(&mut out, theme.clone());
                fmt.write_raw("  ")?;
                fmt.warning(&format!("Could not update factory tooling: {e}"))?;
            }
            String::new()
        }
    };

    // Codex config + built-ins
    let codex_builtins_updated = if codex_enabled {
        if !cli.json {
            let mut out = io::stdout();
            let mut fmt = Formatter::stdout(&mut out, theme.clone());
            fmt.subheading("Syncing .codex files")?;
        }

        match configure_codex_mcp_server(project_root) {
            Ok(true) => {
                codex_config_updated.push("config.toml");
                if !cli.json {
                    let mut out = io::stdout();
                    let mut fmt = Formatter::stdout(&mut out, theme.clone());
                    fmt.write_raw("  ")?;
                    fmt.success("Updated .codex/config.toml")?;
                }
            }
            Ok(false) => {} // No changes needed
            Err(e) => {
                if !cli.json {
                    let mut out = io::stdout();
                    let mut fmt = Formatter::stdout(&mut out, theme.clone());
                    fmt.write_raw("  ")?;
                    fmt.warning(&format!("Could not update config.toml: {e}"))?;
                }
            }
        }

        let codex_result =
            sync_all_builtins_for_harness(cas_mux::SupervisorCli::Codex, &codex_dir)?;
        codex_result.total_updated()
    } else {
        0
    };

    if cli.json {
        let config_json: Vec<String> = config_updated.iter().map(|s| format!("\"{s}\"")).collect();
        let codex_config_json: Vec<String> = codex_config_updated
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect();
        println!(
            r#"{{"config_updated":[{}],"builtins_updated":{},"codex_config_updated":[{}],"codex_builtins_updated":{},"rules_synced":{},"rules_removed":{},"skills_synced":{},"skills_removed":{},"factory_tooling":"{}"}}"#,
            config_json.join(","),
            builtin_result.total_updated(),
            codex_config_json.join(","),
            codex_builtins_updated,
            rule_report.synced,
            rule_report.removed,
            skill_report.synced,
            skill_report.removed,
            factory_tooling_result
        );
    } else {
        let mut out = io::stdout();
        let mut fmt = Formatter::stdout(&mut out, theme);

        // Report built-in updates
        if builtin_result.total_updated() > 0 {
            fmt.write_raw("  ")?;
            fmt.success(&format!(
                "Updated {} built-in files ({} agents, {} skills)",
                builtin_result.total_updated(),
                builtin_result.agents_updated,
                builtin_result.skills_updated
            ))?;
            for file in &builtin_result.updated_files {
                fmt.write_raw(&format!("    + {file}"))?;
                fmt.newline()?;
            }
        } else {
            fmt.write_raw("  ")?;
            fmt.success("Built-ins up to date")?;
        }

        // Report database rule sync
        if rule_report.synced > 0 || rule_report.removed > 0 {
            fmt.write_raw("  ")?;
            fmt.success(&format!(
                "Synced {} rules, removed {}",
                rule_report.synced, rule_report.removed
            ))?;
        } else {
            fmt.write_raw("  ")?;
            fmt.success("Database rules up to date")?;
        }

        // Report database skill sync
        if skill_report.synced > 0 || skill_report.removed > 0 {
            fmt.write_raw("  ")?;
            fmt.success(&format!(
                "Synced {} skills, removed {}",
                skill_report.synced, skill_report.removed
            ))?;
        } else {
            fmt.write_raw("  ")?;
            fmt.success("Database skills up to date")?;
        }
    }

    Ok(())
}

/// Run schema migrations only
fn run_schema_migrations(
    args: &UpdateArgs,
    cli: &Cli,
    cas_root_param: Option<&Path>,
) -> anyhow::Result<()> {
    // cas_root is optional - if not provided, CAS is not initialized
    let cas_root = match cas_root_param {
        Some(path) => path.to_path_buf(),
        None => {
            if cli.json {
                println!(r#"{{"schema_status":"not_initialized","migrations_applied":0}}"#);
            } else {
                let mut out = io::stdout();
                let theme = ActiveTheme::default();
                let mut fmt = Formatter::stdout(&mut out, theme);
                fmt.warning("CAS not initialized in this directory")?;
                fmt.write_raw("  Run ")?;
                fmt.write_accent("cas init")?;
                fmt.write_raw(" to initialize")?;
                fmt.newline()?;
            }
            return Ok(());
        }
    };

    let project_root = cas_root.parent().unwrap_or(&cas_root);

    // Verify the database is initialized before attempting migrations
    let db_path = cas_root.join("cas.db");
    if db_path.exists() {
        let conn = rusqlite::Connection::open(&db_path)?;
        let table_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name IN ('entries', 'rules', 'tasks')",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0);
        if table_count < 3 {
            if cli.json {
                println!(r#"{{"schema_status":"not_initialized","migrations_applied":0}}"#);
            } else {
                let mut out = io::stdout();
                let theme = ActiveTheme::default();
                let mut fmt = Formatter::stdout(&mut out, theme);
                fmt.warning("CAS database not initialized")?;
                fmt.write_raw("  Run ")?;
                fmt.write_accent("cas init")?;
                fmt.write_raw(" to initialize")?;
                fmt.newline()?;
            }
            return Ok(());
        }
    }

    let status = check_migrations(&cas_root)?;

    // Build transaction with all pending changes
    let tx = build_update_transaction(project_root, &cas_root, &status, args.keep_backup);

    if args.dry_run {
        let claude_dir = project_root.join(".claude");
        let codex_dir = project_root.join(".codex");
        return show_enhanced_dry_run(&tx, &status, &claude_dir, &codex_dir, cli);
    }

    if !tx.has_changes() {
        if cli.json {
            println!(
                r#"{{"schema_status":"up_to_date","current_version":{},"migrations_applied":0}}"#,
                status.current_version
            );
        } else {
            let mut out = io::stdout();
            let theme = ActiveTheme::default();
            let mut fmt = Formatter::stdout(&mut out, theme);
            fmt.success(&format!("Schema up to date (v{})", status.current_version))?;
        }
        return Ok(());
    }

    // Create backup before making changes
    let mut tx = tx;
    if !cli.json {
        let mut out = io::stdout();
        let theme = ActiveTheme::default();
        let mut fmt = Formatter::stdout(&mut out, theme);
        fmt.write_accent("\u{2192} ")?;
        fmt.write_raw("Creating backup...")?;
        fmt.newline()?;
    }
    tx.backup()?;
    if let Some(backup_dir) = tx.backup_dir() {
        if !cli.json {
            let mut out = io::stdout();
            let theme = ActiveTheme::default();
            let mut fmt = Formatter::stdout(&mut out, theme);
            fmt.write_raw("  ")?;
            fmt.success(&format!("Backup created at {}", backup_dir.display()))?;
        }
    }

    if !cli.json && tx.migration_count() > 0 {
        let mut out = io::stdout();
        let theme = ActiveTheme::default();
        let mut fmt = Formatter::stdout(&mut out, theme);
        fmt.write_accent("\u{2192} ")?;
        fmt.write_raw(&format!(
            "Running {} schema migration(s)...",
            tx.migration_count()
        ))?;
        fmt.newline()?;
    }

    // Run migrations
    let result = run_migrations(&cas_root, false)?;

    // Check if migrations succeeded
    if !result.errors.is_empty() {
        // Migrations failed - rollback
        if !cli.json {
            let mut out = io::stdout();
            let theme = ActiveTheme::default();
            let mut fmt = Formatter::stdout(&mut out, theme);
            fmt.warning("Migration errors detected, rolling back...")?;
            for (name, error) in &result.errors {
                fmt.write_raw("  ")?;
                fmt.error(&format!("{name} - {error}"))?;
            }
        }
        tx.rollback()?;
        if !cli.json {
            let mut out = io::stdout();
            let theme = ActiveTheme::default();
            let mut fmt = Formatter::stdout(&mut out, theme);
            fmt.success("Rolled back to backup")?;
        }
        anyhow::bail!("Migration failed, changes rolled back");
    }

    // Apply file changes
    if tx.file_change_count() > 0 {
        if !cli.json {
            let mut out = io::stdout();
            let theme = ActiveTheme::default();
            let mut fmt = Formatter::stdout(&mut out, theme);
            fmt.write_accent("\u{2192} ")?;
            fmt.write_raw(&format!(
                "Applying {} file change(s)...",
                tx.file_change_count()
            ))?;
            fmt.newline()?;
        }
        if let Err(e) = tx.apply_file_changes() {
            // File changes failed - rollback
            if !cli.json {
                let mut out = io::stdout();
                let theme = ActiveTheme::default();
                let mut fmt = Formatter::stdout(&mut out, theme);
                fmt.error(&format!("File update failed: {e}"))?;
                fmt.write_accent("\u{2192} ")?;
                fmt.write_raw("Rolling back...")?;
                fmt.newline()?;
            }
            tx.rollback()?;
            if !cli.json {
                let mut out = io::stdout();
                let theme = ActiveTheme::default();
                let mut fmt = Formatter::stdout(&mut out, theme);
                fmt.success("Rolled back to backup")?;
            }
            anyhow::bail!("File update failed, changes rolled back: {e}");
        }
    }

    // Success - commit transaction
    tx.commit()?;

    if cli.json {
        let applied_json: Vec<String> = result
            .applied_names
            .iter()
            .map(|n| format!("\"{n}\""))
            .collect();

        println!(
            r#"{{"schema_status":"updated","current_version":{},"migrations_applied":{},"applied":[{}],"files_updated":{}}}"#,
            status.current_version + result.applied_count as u32,
            result.applied_count,
            applied_json.join(","),
            tx.file_change_count()
        );
    } else {
        let mut out = io::stdout();
        let theme = ActiveTheme::default();
        let mut fmt = Formatter::stdout(&mut out, theme);

        for name in &result.applied_names {
            fmt.write_raw("  ")?;
            fmt.success(name)?;
        }

        fmt.newline()?;
        fmt.success(&format!("Schema updated to v{}", status.latest_version))?;

        if args.keep_backup {
            if let Some(backup_dir) = tx.backup_dir() {
                fmt.write_raw("  ")?;
                fmt.info(&format!("Backup kept at {}", backup_dir.display()))?;
            }
        }
    }

    Ok(())
}

/// Check if a newer version is available (binary + schema)
fn check_for_updates(
    current_version: &str,
    cli: &Cli,
    cas_root_param: Option<&Path>,
) -> anyhow::Result<()> {
    use self_update::backends::github::Update;

    // Check binary updates
    let mut builder = Update::configure();
    builder
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .current_version(current_version);
    if let Some(token) = github_auth_token() {
        builder.auth_token(&token);
    }
    let updater = builder.build()?;

    let latest = updater.get_latest_release()?;
    let latest_version = latest.version.trim_start_matches('v');
    let binary_update_available = is_newer(latest_version, current_version);

    // Check schema migrations - only if cas_root is provided (CAS is initialized)
    let schema_status = cas_root_param.and_then(|path| check_migrations(path).ok());

    let pending_migrations = schema_status.as_ref().map(|s| s.pending.len()).unwrap_or(0);

    if cli.json {
        println!(
            r#"{{"current_version":"{}","latest_version":"{}","binary_update_available":{},"schema_version":{},"pending_migrations":{}}}"#,
            current_version,
            latest_version,
            binary_update_available,
            schema_status
                .as_ref()
                .map(|s| s.current_version)
                .unwrap_or(0),
            pending_migrations
        );
        return Ok(());
    }

    let mut out = io::stdout();
    let theme = ActiveTheme::default();
    let mut fmt = Formatter::stdout(&mut out, theme);

    fmt.subheading("Binary")?;
    fmt.write_raw("  Current version: ")?;
    fmt.write_accent(current_version)?;
    fmt.newline()?;
    fmt.write_raw("  Latest version:  ")?;
    fmt.write_accent(latest_version)?;
    fmt.newline()?;

    if binary_update_available {
        fmt.newline()?;
        let success_color = fmt.theme().palette.status_success;
        fmt.write_colored("  \u{2192} ", success_color)?;
        fmt.write_raw("A new version is available!")?;
        fmt.newline()?;
        fmt.write_raw("    Run ")?;
        fmt.write_accent("cas update")?;
        fmt.write_raw(" to update")?;
        fmt.newline()?;
    } else {
        fmt.newline()?;
        fmt.write_raw("  ")?;
        fmt.success("Binary up to date")?;
    }

    fmt.newline()?;
    fmt.subheading("Schema")?;

    if let Some(status) = schema_status {
        fmt.write_raw("  Current version: ")?;
        fmt.write_accent(&format!("v{}", status.current_version))?;
        fmt.newline()?;
        fmt.write_raw("  Latest version:  ")?;
        fmt.write_accent(&format!("v{}", status.latest_version))?;
        fmt.newline()?;

        if pending_migrations > 0 {
            let warning_color = fmt.theme().palette.status_warning;
            fmt.newline()?;
            fmt.write_colored("  \u{2192} ", warning_color)?;
            fmt.write_raw(&format!("{pending_migrations} migration(s) pending"))?;
            fmt.newline()?;
            fmt.write_raw("    Run ")?;
            fmt.write_accent("cas update --dry-run")?;
            fmt.write_raw(" to preview")?;
            fmt.newline()?;
            fmt.write_raw("    Run ")?;
            fmt.write_accent("cas update --schema-only")?;
            fmt.write_raw(" to apply")?;
            fmt.newline()?;
        } else {
            fmt.newline()?;
            fmt.write_raw("  ")?;
            fmt.success("Schema up to date")?;
        }
    } else {
        fmt.write_raw("  ")?;
        fmt.warning("CAS not initialized in this directory")?;
    }

    Ok(())
}

/// Download and install the latest (or specified) version
fn perform_update(args: &UpdateArgs, current_version: &str, cli: &Cli) -> anyhow::Result<()> {
    use self_update::Status;
    use self_update::backends::github::Update;

    let mut updater = Update::configure();
    updater
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name(BIN_NAME)
        .current_version(current_version)
        .show_download_progress(true)
        .no_confirm(args.yes);
    if let Some(token) = github_auth_token() {
        updater.auth_token(&token);
    }

    // If a specific version is requested, set it
    if let Some(ref version) = args.version {
        updater.target_version_tag(&format!("v{}", version.trim_start_matches('v')));
    }

    let updater = updater.build()?;

    // Check what we're updating to
    let latest = updater.get_latest_release()?;
    let target_version = args
        .version
        .as_ref()
        .map(|v| v.trim_start_matches('v').to_string())
        .unwrap_or_else(|| latest.version.trim_start_matches('v').to_string());

    if !args.yes && !cli.json {
        let mut out = io::stdout();
        let theme = ActiveTheme::default();
        let mut fmt = Formatter::stdout(&mut out, theme);

        fmt.subheading("Binary Update")?;
        fmt.write_raw("  Current version: ")?;
        fmt.write_accent(current_version)?;
        fmt.newline()?;
        fmt.write_raw("  Target version:  ")?;
        fmt.write_accent(&target_version)?;
        fmt.newline()?;

        if !is_newer(&target_version, current_version) && args.version.is_none() {
            fmt.newline()?;
            fmt.write_raw("  ")?;
            fmt.success("Already on the latest version")?;
            return Ok(());
        }

        fmt.newline()?;
        fmt.write_raw("  This will download and replace the current binary.")?;
        fmt.newline()?;
    }

    // Perform the update
    let status = updater.update()?;

    if cli.json {
        let (updated, version) = match &status {
            Status::UpToDate(v) => (false, v.as_str()),
            Status::Updated(v) => (true, v.as_str()),
        };
        println!(
            r#"{{"binary_updated":{},"version":"{}"}}"#,
            updated,
            version.trim_start_matches('v')
        );
        return Ok(());
    }

    let mut out = io::stdout();
    let theme = ActiveTheme::default();
    let mut fmt = Formatter::stdout(&mut out, theme);

    match status {
        Status::UpToDate(v) => {
            fmt.newline()?;
            fmt.write_raw("  ")?;
            fmt.success(&format!(
                "Already up to date ({})",
                v.trim_start_matches('v')
            ))?;
        }
        Status::Updated(v) => {
            fmt.newline()?;
            fmt.write_raw("  ")?;
            fmt.success(&format!(
                "Successfully updated to {}",
                v.trim_start_matches('v')
            ))?;
            fmt.newline()?;
            fmt.write_raw("  Run ")?;
            fmt.write_accent("cas changelog")?;
            fmt.write_raw(" to see what's new")?;
            fmt.newline()?;
        }
    }

    Ok(())
}

/// Try to get a GitHub auth token from `gh auth token` or GITHUB_TOKEN env var.
fn github_auth_token() -> Option<String> {
    // Try GITHUB_TOKEN env var first
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            return Some(token);
        }
    }
    // Fall back to `gh auth token`
    std::process::Command::new("gh")
        .args(["auth", "token"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            } else {
                None
            }
        })
}

/// Compare semantic versions to check if `new` is newer than `current`
fn is_newer(new: &str, current: &str) -> bool {
    let parse = |v: &str| -> Option<(u32, u32, u32)> {
        let parts: Vec<&str> = v.trim_start_matches('v').split('.').collect();
        if parts.len() >= 3 {
            Some((
                parts[0].parse().ok()?,
                parts[1].parse().ok()?,
                parts[2].split('-').next()?.parse().ok()?,
            ))
        } else {
            None
        }
    };

    match (parse(new), parse(current)) {
        (Some((n1, n2, n3)), Some((c1, c2, c3))) => (n1, n2, n3) > (c1, c2, c3),
        _ => false,
    }
}

struct UpdateStepTracker {
    total: usize,
    current: usize,
    enabled: bool,
}

impl UpdateStepTracker {
    fn new(total: usize, enabled: bool) -> Self {
        Self {
            total,
            current: 0,
            enabled,
        }
    }

    fn run<T, F>(&mut self, label: &str, f: F) -> anyhow::Result<T>
    where
        F: FnOnce() -> anyhow::Result<T>,
    {
        let step_num = self.current + 1;
        let started_at = Instant::now();

        if self.enabled {
            let mut out = io::stdout();
            let theme = ActiveTheme::default();
            let mut fmt = Formatter::stdout(&mut out, theme);
            fmt.write_accent("\u{2192} ")?;
            fmt.write_raw(&format!("[{}/{}] ", step_num, self.total))?;
            fmt.write_bold(label)?;
            fmt.newline()?;
        }

        match f() {
            Ok(value) => {
                if self.enabled {
                    let mut out = io::stdout();
                    let theme = ActiveTheme::default();
                    let mut fmt = Formatter::stdout(&mut out, theme);
                    fmt.write_raw("  ")?;
                    fmt.success(&format!(
                        "{label} ({})",
                        format_elapsed(started_at.elapsed())
                    ))?;
                }
                self.current += 1;
                Ok(value)
            }
            Err(err) => {
                if self.enabled {
                    let mut out = io::stdout();
                    let theme = ActiveTheme::default();
                    let mut fmt = Formatter::stdout(&mut out, theme);
                    fmt.write_raw("  ")?;
                    fmt.error(&format!(
                        "{label} ({})",
                        format_elapsed(started_at.elapsed())
                    ))?;
                }
                Err(err)
            }
        }
    }
}

fn format_elapsed(duration: Duration) -> String {
    if duration.as_secs() >= 60 {
        let mins = duration.as_secs() / 60;
        let secs = duration.as_secs() % 60;
        format!("{mins}m {secs}s")
    } else if duration.as_millis() >= 1000 {
        format!("{:.1}s", duration.as_secs_f64())
    } else {
        format!("{}ms", duration.as_millis())
    }
}

#[cfg(test)]
#[path = "update_tests/tests.rs"]
mod tests;
