# OpenCode Agent Instructions for Alchemy

Alchemy is a cross-platform CI/CD AI agent written in Rust. Single static binary, dual-mode: pipe mode for automation and TUI mode for interactive use.

## Build, Test & Lint Commands

- **Build**: `cargo build --release` (Produces a single static binary. Uses `opt-level = "z"`, LTO, single codegen unit).
- **Lint**: `cargo clippy --all-targets -- -D warnings` (Must run cleanly, includes tests).
- **Unit Tests**: `cargo test`
- **E2E Tests**: `ALCHEMY_E2E=1 cargo test` (Requires a real LLM API key).

## Architecture & Execution Flow

- **Entrypoints**: The core loop (`src/agent.rs`) is frontend-agnostic. Three frontends wrap it in `src/main.rs`: Pipe mode, TUI mode, and Concourse CI adapters.
- **Provider API**: The agent loop uses `chat_streaming` exclusively. The standard `chat` function is reserved for RAG embedding and single-shot calls.
- **Context Compaction**:
  - `compact_context` (in-turn): Keeps last 12 messages + system prompt.
  - `compact_history` (TUI mode): Keeps last 12 history messages.
  - Context is compacted at 85% of `ALCHEMY_CONTEXT_WINDOW`. **No summarization is performed**.
  - History **must always end with an assistant message**. Both normal and error returns push an assistant message.
- **Tool Dispatch**:
  - Built-in tools have no prefix (e.g., `read_file`, `execute_cmd`).
  - MCP tools use the prefix `mcp_<server>_<tool>`.
  - Skills use the prefix `skill_<name>_<script>`.
- **Parallel Execution**: Only `read_file`, `list_dir`, and `fetch_url` are `is_parallel_safe = true`. All other tools (including MCP and Skills) run sequentially.
- **Skills System**: Skills activate if ≥2 words from their description (>3 chars) match the prompt or ecosystem marker files in cwd.
- **Concourse CI**: The `check` script uses the SHA-256 hash of the agent's answer as the version `ref`. `in` stores `response.txt`, `response.json`, and `metadata.json`.

## Critical Constraints & Gotchas

- **No OS-specific code in core**: `#[cfg(target_os)]` is strictly banned. The *only* exceptions are terminal handling (`src/tui/`) and shell execution wrappers in `builtin.rs::execute_cmd`.
- **Configuration**: Purely environment-variable-driven (no config files in pipe mode). `ALCHEMY_PROVIDER` is always required.
- **RAG Embedding Requirements**: RAG is powered by bundled SQLite with a statically linked `sqlite-vec` extension. Embedding-capable providers are `openai`, `gemini`, `ollama`, `github-copilot`, and `openrouter`. `ALCHEMY_RAG_EMBED_PROVIDER` defaults to `ALCHEMY_PROVIDER`, so it only needs setting when the chat provider cannot embed (e.g. `anthropic`); otherwise the binary exits with code 2.
- **RAG embedding credentials are separate**: the embedding provider resolves its key from `ALCHEMY_RAG_EMBED_API_KEY` and its endpoint from `ALCHEMY_RAG_EMBED_BASE_URL`, each falling back to `ALCHEMY_API_KEY` / no-override. Chat's `ALCHEMY_BASE_URL` is never used for embeddings. When `ALCHEMY_RAG_EMBED_PROVIDER` differs from `ALCHEMY_PROVIDER` without a dedicated embed key, a warning is logged because reusing the chat key will usually fail auth.
- **OpenRouter model IDs are namespaced**: OpenRouter requires `<vendor>/<model>` (e.g. `openai/text-embedding-3-small`), so the bare OpenAI-style name is invalid there. Embedding width is resolved per model, with `ALCHEMY_RAG_DIMENSIONS` overriding. Changing dimensions drops existing embeddings on migration — re-run `alchemy rag index`.
- **TUI Logging**: In TUI mode, logs MUST go to `ALCHEMY_LOG_FILE` (`~/.alchemy/debug.log`). Writing to stderr will corrupt the ratatui display.
- **Exit Codes**: 
  - `0`: Success
  - `1`: Agent / runtime failure (provider error, max steps exceeded)
  - `2`: Configuration error (missing env var, bad provider for RAG, no prompt)

## Reference Files

- `SPEC.md`: Full product specification and behaviors.
- `.github/copilot-instructions.md` / `CLAUDE.md`: Additional baseline instructions.
