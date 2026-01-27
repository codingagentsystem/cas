# CAS — Run a Team of Claude Code Agents

One supervisor plans. Multiple workers execute. You review the results.

## Install

```bash
curl -fsSL https://cas.dev/install.sh | sh
```

Then in your project:

```bash
cas init          # Initialize CAS
cas factory       # Launch Factory Mode
```

## The Factory Pattern

| Step | Role | What It Does |
|------|------|--------------|
| 01 | **Supervisor** | Breaks down requests into EPICs with discrete tasks and dependencies |
| 02 | **Workers** | Claim tasks and work simultaneously in isolated git worktrees |
| 03 | **You** | Review PRs and merge when ready |

## Features

| Feature | Description |
|---------|-------------|
| **Persistent Memory** | What one agent learns, all agents remember. Context that compounds across sessions. |
| **Task Orchestration** | Structured task management with dependencies, priorities, and automatic assignment. |
| **Self-Improving Rules** | Patterns that work get promoted. Mistakes become guardrails automatically. |
| **Reusable Skills** | Teach once, every worker knows it. Custom tools, workflows, domain knowledge. |
| **Git Worktrees** | Each worker operates in isolation. Clean parallel development, no merge conflicts. |
| **Team Sync** | Share learnings across your organization. Team-wide memory that benefits everyone. |

## Links

- [Documentation](https://cas.dev/docs)
- [How It Works](https://cas.dev/how-it-works)
- [Blog](https://cas.dev/blog)

---

Built with Rust. Runs locally. Your code stays private.
