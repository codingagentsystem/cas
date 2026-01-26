# CAS — Persistent Context for AI Agents

Your AI agent forgets everything when the session ends. CAS fixes that.

CAS is the context layer for AI coding agents. Memories that persist. Tasks that coordinate. Rules that evolve. Skills that compound. Agents that actually learn.

## Install

```bash
curl -fsSL https://codingagentsystem.com/install.sh | sh
```

## What CAS Does

| Feature | What It Does |
|---------|--------------|
| **Memory** | Stores learnings, preferences, and observations across sessions |
| **Tasks** | Tracks work with dependencies, priorities, and structured notes |
| **Rules** | Coding conventions that auto-sync to Claude Code |
| **Skills** | Reusable agent capabilities with usage tracking |
| **Factory** | Multi-agent coordination with supervisor + workers |
| **Search** | Hybrid BM25 + semantic retrieval across all context |

## Quick Start

### For Agents (MCP Tools)

CAS exposes 57+ tools via MCP. When your agent has CAS access:

```
mcp__cas__memory action=remember content="This codebase uses Zod for validation"
mcp__cas__task action=create title="Implement auth" priority=1 start=true
mcp__cas__search action=search query="error handling patterns"
```

### For Humans (CLI)

```bash
# Initialize in your project
cas init

# Store a learning
cas add "Always use async/await over callbacks" --tags patterns

# Create a task
cas task create "Fix login bug" --priority 1 --start

# Search context
cas search "authentication flow"

# Start MCP server
cas serve
```

### Claude Code Integration

Add to your MCP settings (`~/Library/Application Support/Claude/claude_desktop_config.json`):

```json
{
  "mcpServers": {
    "cas": {
      "command": "cas",
      "args": ["serve"]
    }
  }
}
```

## Factory Mode

Scale beyond single-agent. Factory mode coordinates multiple Claude instances working in parallel.

```bash
# Start supervisor (coordinates work)
cas factory --supervisor

# Spawn workers (in TUI or via MCP)
mcp__cas__factory action=spawn_workers count=3
```

**How it works:**
- **Supervisor** breaks EPICs into tasks, assigns work, reviews PRs
- **Workers** claim tasks, execute in isolated git worktrees, report progress
- **Shared state** via CAS database — all agents see the same memories, tasks, rules

**When to use:**
- Large features requiring parallel implementation
- Site-wide refactors across many files
- Multi-stage workflows with dependencies

## Features

### Memory System

Memories persist across sessions with intelligent retrieval:

```bash
cas add "PostgreSQL with JSONB columns" --tags db,architecture
cas add "Prefer explicit error handling" --type preference
cas search "database patterns" --boost-feedback
```

**Memory Tiers:**
- **In-Context** — Pinned, always injected
- **Working** — Active and searchable (default)
- **Cold** — Less accessed, may be compressed
- **Archive** — Historical reference

### Task System

Dependencies, priorities, structured progress tracking:

```bash
cas task create "Deploy to prod" --blocked-by cas-a1b2
cas task start cas-c3d4
cas task notes cas-c3d4 "Found root cause" --type discovery
cas task close cas-c3d4 --reason "Fixed in abc123"
cas task ready  # Show unblocked tasks
```

### Rules System

Coding conventions that earn trust through use:

```
Draft → Proven → Retired
       ↑
  (cas rules helpful)
```

Proven rules sync to `.claude/rules/` and load into Claude Code automatically.

```bash
cas rules add "Always validate input before DB queries" --paths "**/*.ts"
cas rules helpful rule-001  # Promotes to Proven
```

### Skills System

Reusable capabilities your agent can invoke:

```bash
cas skill create "Format Code" \
  --description "Run prettier on project" \
  --invocation "npx prettier --write ." \
  --type command

cas skill enable skill-001  # Sync to Claude Code
```

### Search

Hybrid retrieval combining keyword matching and semantic understanding:

```bash
cas search "error handling"           # Hybrid search
cas search "testing" --no-semantic    # Fast BM25 only
cas search "patterns" --boost-feedback  # Rank by helpfulness
```

## Architecture

```
.cas/
├── cas.db          # SQLite — memories, tasks, rules, skills
├── config.yaml     # Project configuration
├── vectors.hnsw    # Semantic search index (HNSW)
└── indexes/
    └── tantivy/    # Full-text search index (BM25)
```

**Two-tier storage:**
- **Global** (`~/.config/cas/`) — User preferences, cross-project learnings
- **Project** (`.cas/`) — Project-specific context

**Built with:**
- Rust for performance
- candle for GPU-accelerated embeddings
- tantivy for full-text search
- usearch for vector indexing
- rmcp for MCP protocol

## Documentation

- [Getting Started Guide](docs/GETTING_STARTED.md)
- [Factory Mode Guide](docs/FACTORY_MODE.md)
- [MCP Tools Reference](docs/MCP_TOOLS.md)
- [Configuration Reference](docs/CONFIGURATION.md)

## Self-Update

```bash
cas update
```

## Privacy

CAS collects anonymous usage data to improve the product. No code or personal information is collected.

Opt out: `cas config telemetry.enabled false`

See [Privacy Policy](docs/PRIVACY.md) for details.

## License

Proprietary — All rights reserved.
