# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

Alchemy is a cross-platform CI/CD AI agent written in Rust. Single static binary, dual-mode: pipe mode for automation and TUI mode for interactive use. See `SPEC.md` for the full specification.

## Build & Test

```bash
cargo build --release        # produces single static binary
cargo test                   # run all tests
cargo test <test_name>       # run a single test
cargo clippy -- -D warnings  # lint
ALCHEMY_E2E=1 cargo test     # include E2E tests (requires real LLM API key)
```

## Architecture

The agent core is frontend-agnostic — pipe mode, TUI, and Concourse CI are adapters over the same core in `src/agent.rs`.

**Key modules:**
- `src/agent.rs` — agent loop: LLM call → tool dispatch → repeat until no tool calls
- `src/providers/` — LLM provider trait + implementations (OpenAI-compatible, Copilot, Gemini, Anthropic)
- `src/tools/` — tool registry merging built-in + MCP + skill tools; 5 built-ins: `read_file`, `write_file`, `list_dir`, `execute_cmd`, `fetch_url`
- `src/skills/` — Agent Skills Specification loader; skills extend the system prompt and register callable scripts
- `src/rag/` — optional RAG pipeline (chunker → embedder → vector store → retriever); disabled by default
- `src/tui/` — ratatui + crossterm TUI; only place with platform-specific terminal handling
- `src/concourse/` — thin adapters for Concourse CI `check`/`in`/`out` lifecycle

**Tool namespacing:** `read_file` (built-in), `mcp_<server>_<tool>` (MCP), `skill_<name>_<script>` (skills).

**Streaming:** the agent loop always uses `chat_streaming`; `chat` is only for RAG embedding and single-shot calls.

**Parallel tool execution:** `read_file`, `list_dir`, `fetch_url` may run concurrently; `write_file`, `execute_cmd`, MCP, and skill tools run sequentially.

## Critical Constraints

- **No `#[cfg(target_os)]` in core logic** — shell invocation (`sh -c` vs `cmd /C` in `execute_cmd`) and terminal handling in `src/tui/` are the only exceptions.
- **No sandbox** — tools execute directly; the CI runner is the sandbox.
- **No confirmation in pipe mode** — all tools auto-execute.
- **Configuration is environment-variable-driven** — `ALCHEMY_PROVIDER` is always required; `ALCHEMY_API_KEY` required for remote providers. No config file discovery in pipe mode.
- **Exit codes matter** — 0: success, 1: agent/runtime failure, 2: configuration error.
- **`sqlite-vec` must be statically linked** — no external `.so`/`.dll` at runtime.

## Context Compaction

Triggered at 85% of `ALCHEMY_CONTEXT_WINDOW`. Keeps: system prompt + last 6 logical turns. No summarization — older turns are dropped.

## RAG Embedding Providers

Only `openai`, `gemini`, and `ollama` support embeddings. If RAG is enabled with `anthropic`, `github-copilot`, or `openrouter`, `ALCHEMY_RAG_EMBED_PROVIDER` must be set explicitly or exit with code 2.
