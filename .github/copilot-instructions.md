# Alchemy — Copilot Instructions

Alchemy is a cross-platform CI/CD AI agent written in Rust. Single static binary, dual-mode: pipe mode for automation and TUI mode for interactive use.

## Build, Test & Lint

```bash
cargo build --release          # produces single static binary
cargo test                     # run all tests
cargo test <test_name>         # run a single test by name (substring match)
cargo clippy --all-targets -- -D warnings  # lint, including test code
ALCHEMY_E2E=1 cargo test       # include E2E tests (requires a real LLM API key)
```

Release profile uses `opt-level = "z"`, LTO, single codegen unit, `panic = "abort"`, and `strip = true`.

## Architecture

The agent core (`src/agent.rs`) is frontend-agnostic. Three frontends in `src/main.rs` wrap it:

- **Pipe mode** (`run_pipe`) — reads prompt from args/stdin, runs one turn, prints JSON or text to stdout, exits.
- **TUI mode** (`run_tui`) — interactive ratatui terminal; multi-turn with session persistence.
- **Concourse CI** (`run_concourse_*`) — thin `check`/`in`/`out` adapters over the same agent loop.

### Agent loop (`src/agent.rs`)

`run_internal` owns the `messages: Vec<Message>` for the lifetime of one turn:

1. Build `[system, ...history, user_msg]`.
2. Call `compact_context` (drops oldest messages if >85% of context window).
3. Call the LLM via `stream_with_callback`.
4. If no tool calls → push assistant message, return `(AgentResult, messages[1..])`.
5. If tool calls → dispatch tools (parallel or sequential), push results, loop.

The returned `messages[1..]` slice (history without the system prompt) is stored in `TuiApp::conversation_history` and passed as `history` to the next turn. **History must always end with an assistant message**; both error-return paths (`max_steps` exceeded, provider error) explicitly push a bracketed error string as an assistant message before returning.

### TUI event loop (`src/tui/mod.rs`)

Single `loop` per frame (50 ms poll):

1. Drain mpsc channels: tool events, file events, streaming tokens, step events.
2. Check oneshot `pending` — if agent turn completed, update `conversation_history`, push final message.
3. `terminal.draw(...)`.
4. `event::poll(50ms)` → filter to `KeyEventKind::Press` only (crossterm on Windows also emits `Release`/`Repeat`).

The background agent task communicates via:
- `oneshot` → final `(AgentResult, Vec<Message>)`
- `mpsc` → streaming tokens, tool events, file events, step/token-count progress

### Provider trait (`src/providers/mod.rs`)

```rust
trait Provider: Send + Sync {
    fn chat_streaming(&self, request, tx) -> Result<LlmResponse>;
    fn chat(&self, request) -> Result<LlmResponse>  // default: stream + drain
    fn embed(&self, texts) -> Result<Vec<Vec<f32>>>  // default: bail!
}
```

The agent loop always uses `chat_streaming`; `chat` is only for RAG embedding. New providers implement `chat_streaming` only.

### Tool registry (`src/tools/mod.rs`)

Tool name prefix determines dispatch:
- No prefix → built-in (`read_file`, `write_file`, `list_dir`, `execute_cmd`, `fetch_url`)
- `mcp_<server>_<tool>` → MCP
- `skill_<name>_<script>` → skill script

`is_parallel_safe` returns `true` only for `read_file`, `list_dir`, `fetch_url`. All others run sequentially.

### Skills (`src/skills/mod.rs`)

Each skill is a directory under `ALCHEMY_SKILLS_DIR` (default `~/.alchemy/skills`) containing:
- `SKILL.md` — YAML frontmatter (`name`, `description`) + body injected into system prompt on activation
- `scripts/` — executable scripts registered as `skill_<name>_<script_name>` tools
- `references/` and `assets/` — read-only resources surfaced via a per-skill `read_resource` tool

Skills are matched by counting description words (>3 chars) that appear in the lowercased prompt + cwd ecosystem tokens (inferred from marker files like `Cargo.toml`, `package.json`, etc.). A skill activates when ≥2 words match.

### System prompt construction

In `main.rs::run_pipe` (and `run_tui`):
```
final_system = base_system_prompt + skill_context + rag_context
```
`skill_context` is built from matched skills' SKILL.md bodies. `rag_context` is retrieved chunks prepended to the prompt.

### RAG (`src/rag/`)

Disabled by default (`ALCHEMY_RAG_ENABLED=true` to enable). Uses SQLite (bundled) with brute-force cosine similarity in Rust — `sqlite-vec` is **not yet wired in** despite the TODO. Only `openai`, `gemini`, and `ollama` providers support embeddings; using another provider requires `ALCHEMY_RAG_EMBED_PROVIDER` set explicitly or the binary exits with code 2.

## Key Conventions

**No `#[cfg(target_os)]` in core logic.** The two legitimate exceptions are shell selection (`sh -c` vs `cmd /C`) in `builtin.rs::execute_cmd` and terminal setup in `src/tui/`.

**Exit codes are meaningful:**
- `0` — success
- `1` — agent/runtime failure
- `2` — configuration error (missing env var, bad provider for RAG, no prompt)

**Configuration is entirely env-var driven.** Core variables:

| Variable | Required | Default | Notes |
|---|---|---|---|
| `ALCHEMY_PROVIDER` | ✓ | — | `openai`, `github-copilot`, `anthropic`, `gemini`, `ollama`, `openrouter` |
| `ALCHEMY_API_KEY` | for remote | — | Not needed for `ollama` |
| `ALCHEMY_MODEL` | — | provider default | |
| `ALCHEMY_BASE_URL` | — | provider default | Override endpoint |
| `ALCHEMY_SYSTEM_PROMPT` | — | built-in default | |
| `ALCHEMY_MAX_STEPS` | — | `30` | |
| `ALCHEMY_TIMEOUT_SECS` | — | `30` | Per-tool timeout |
| `ALCHEMY_CONTEXT_WINDOW` | — | `128000` | Tokens; compaction at 85% |
| `ALCHEMY_OUTPUT` | — | `json` | `json` or `text` |
| `ALCHEMY_MCP_SERVERS` | — | — | JSON array of server configs |
| `ALCHEMY_MCP_CONFIG` | — | — | Path to JSON config file |
| `ALCHEMY_SKILLS_ENABLED` | — | `true` | Set `false` to disable |
| `ALCHEMY_SKILLS_DIR` | — | `~/.alchemy/skills` | |
| `ALCHEMY_RAG_ENABLED` | — | `false` | |
| `ALCHEMY_RAG_EMBED_PROVIDER` | if RAG + non-embed provider | — | `openai`, `gemini`, or `ollama` |
| `ALCHEMY_SESSION_DIR` | — | `~/.alchemy/sessions` | TUI session storage |
| `ALCHEMY_LOG_FILE` | — | `~/.alchemy/debug.log` | TUI mode only |

**TUI logs must go to a file**, not stderr — writing to stderr corrupts the ratatui display. `main.rs` redirects tracing to `ALCHEMY_LOG_FILE` (default `~/.alchemy/debug.log`) when running in TUI mode.

**Context compaction has two layers:**
- `compact_context` — called at the top of each agent step (in-turn); keeps last 12 messages + system prompt
- `compact_history` — called after each TUI turn completes; keeps last 12 history messages

**`sqlite-vec` must be statically linked** if/when it gets wired in — no external `.so`/`.dll` at runtime.

**No confirmation in pipe mode.** All tools auto-execute; the CI runner is the sandbox.

**Concourse `check` uses SHA-256 of the agent's answer as the version `ref`.**  `in` stores `answer.txt`, `metadata.json`, `steps`, and `tools_used` files in the destination directory.
