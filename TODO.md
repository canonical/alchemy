# TODO — Feature Status

## RAG Pipeline

- [x] **Real embedding providers** — OpenAI, Gemini, Ollama, GitHub Copilot, and OpenRouter embedders implemented in `embedder.rs` (5 providers total; Copilot and OpenRouter use the OpenAI-compatible embeddings endpoint)
- [x] **Embedding provider selection** — `main.rs` selects embed provider based on `ALCHEMY_RAG_EMBED_PROVIDER` / `ALCHEMY_PROVIDER`; validation exits with code 2 when non-embedding provider is used without an explicit `ALCHEMY_RAG_EMBED_PROVIDER`
- [x] **Reranking** — `retriever.rs` implements MMR (Maximal Marginal Relevance) reranking. Note: per-chunk embeddings are not persisted, so inter-candidate similarity is approximated via relevance-score proximity rather than true embedding-space distance.
- [x] **`sqlite-vec` extension** — `sqlite-vec` v0.1.9 statically linked via `cc::Build` + `sqlite3_auto_extension`; `store.rs` uses a `vec0` virtual table with `distance_metric=cosine`; auto-migrates old BLOB-schema DBs on open (chunks content preserved, embeddings dropped with a warning)
- [x] **External vector store backends** — Qdrant and Chroma HTTP backends implemented. `VectorStoreBackend` trait in `store.rs` abstracts over SQLite/Qdrant/Chroma. Backend selected via `ALCHEMY_RAG_STORE=sqlite|qdrant|chroma` (default: `sqlite`). Qdrant/Chroma require `ALCHEMY_RAG_STORE_URL`; optional `ALCHEMY_RAG_STORE_API_KEY` and `ALCHEMY_RAG_STORE_COLLECTION` (default: `"alchemy"`).
- [x] **Separate embedding credentials** — `ALCHEMY_RAG_EMBED_API_KEY` and `ALCHEMY_RAG_EMBED_BASE_URL` configure the embedding provider independently of the chat provider (both fall back to the chat values). The chat `ALCHEMY_BASE_URL` no longer leaks into embedding calls, and a warning is logged when the embed provider differs from the chat provider without a dedicated key.
- [x] **OpenRouter embedding model IDs** — OpenRouter requires namespaced IDs, so the default embed model is `openai/text-embedding-3-small` rather than the bare OpenAI name.
- [x] **Per-model embedding dimensions** — `embed_dimensions_for_model` resolves vector width from the model name (1536/3072/1024/768/384), falling back to the provider default for unknown models; `ALCHEMY_RAG_DIMENSIONS` overrides.

## TUI Mode

### Navigation & Scrolling
- [x] **Panel focus cycling** — `Tab` cycles focus between Conversation / Tools / Files panels
- [x] **Scroll support** — `PageUp/PageDown`, `Alt+↑/↓`, and `Ctrl+↑/↓` scroll the focused panel; bare `↑/↓` navigates prompt history in the input box
- [x] **Home / End** — When input is empty, scrolls focused panel to top/bottom; when input has content, moves cursor to start/end
- [x] **Panel toggles** — `Alt+T` hides/shows Tools panel; `Alt+F` hides/shows Files panel; state persists to `~/.config/alchemy/panels`

### I/O & Session
- [x] **Typewriter/streaming effect** — Streaming tokens accumulated incrementally via `streaming_content`
- [x] **Ctrl+C interrupt** — Aborts running agent task via `abort_handle`
- [x] **Ctrl+L clear** — Clears conversation, tools log, files log, and `context.json`/`messages.jsonl` on disk; next launch starts fresh
- [x] **Ctrl+S manual save** — Saves session messages to disk
- [x] **Ctrl+D exit** — Exits TUI
- [x] **Persistent prompt history** — Global `~/.alchemy/prompt_history`; loads last 1000 entries on startup; `↑/↓` navigates; new entries appended asynchronously
- [x] **CWD-based session naming** — Session directory derived from `slugify(cwd.file_name())` so each project folder has its own isolated history
- [x] **LLM context persistence** — `context.json` saved after each turn; restored on next launch so the conversation resumes seamlessly without resending history
- [x] **AGENTS.md auto-load (TUI)** — Prompts `[Y/n]` before raw mode; accepted content prepended to system prompt

### Display
- [x] **Syntax highlighting** — `src/tui/markdown.rs` renders assistant messages with headings, bold/italic, inline + fenced code, lists, block quotes, and rules
- [x] **Tool execution spinner animation** — In-progress tool entries animate while running
- [x] **File activity fade animation** — New file entries fade yellow → green → default over ~1s
- [x] **Token count real-time updates** — Per-step `StepEvent` channel drives live status-bar updates; authoritative final value on turn end
- [x] **Theme cycling** — `Alt+C` cycles Dark → Light → Dracula → Solarized; persists to `~/.config/alchemy/theme`
- [x] **Turn-annotated panels** — Tools and Files panels group entries by turn with `#N HH:MM:SS` header; persist across turns; Ctrl+L resets
- [x] **MCP / Skill badges** — Tool entries in the Tools panel show `[M]` (cyan) for MCP tools and `[S]` (green) for Skill tools

### Overlays
- [x] **Help overlay** — `?` opens a full keybinding reference
- [x] **Skills overlay** — `Alt+S` shows loaded skills with descriptions
- [x] **MCP overlay** — `Alt+M` shows connected MCP servers and their tools
- [x] **Shift+Enter multiline input** — Inserts a newline at the cursor; input box grows up to 4 rows; `draw_input` renders each line with `> ` / `  ` prefix; cursor positioned correctly across rows

## Skills System

- [x] **Basic skill loading and trigger matching** — Skill loader, registry, and keyword trigger matching implemented
- [x] **`ALCHEMY_SKILLS_ENABLED=false`** — Skills can be disabled via env var
- [x] **Full SKILL.md body loaded on activation** — `build_skill_context()` reads and appends full body to system prompt
- [x] **Script discovery** — Scripts in `scripts/` dir discovered and registered as skill tools
- [x] **File pattern inference** — `cwd_signals()` derives ecosystem tokens from marker files
- [x] **Progressive disclosure** — Metadata at startup, instructions on activation, resources on-demand
- [x] **References/assets loading on demand** — Surfaced via per-skill `read_resource` tool with path-allowlist validation

## MCP Client

- [x] **SSE transport** — Both `stdio` and `sse` transports implemented with full JSON-RPC protocol
- [x] **Graceful degradation** — Connection failures logged with `tracing::warn!` and skipped
- [x] **Initialize + notifications/initialized handshake** — Proper MCP protocol initialization on both transports
- [x] **Mid-session crash recovery** — SSE stream errors drain pending oneshots with descriptive error; new calls fail fast via `closed` atomic flag

## Concourse CI

- [x] **CLI subcommand dispatch** — `check`/`in`/`out` entrypoints dispatched from `main.rs`
- [x] **Metadata completeness** — `in_cmd.rs` and `out.rs` emit `model`, `duration_secs`, and `tools_used` in both `metadata.json` and the Concourse `metadata[]` array

## CLI / Pipe Mode

- [x] **`--system` flag** — Prepends custom system prompt
- [x] **`--version` flag** — Implemented via clap `#[command(version)]`; reads from `Cargo.toml` via `env!("CARGO_PKG_VERSION")`
- [x] **`--yes` / `-y` flag** — Auto-loads `AGENTS.md` silently without prompting
- [x] **`--max-steps`** — Limits agent loop iterations
- [x] **`--timeout`** — Per-step timeout in seconds
- [x] **Exit code 2 for config errors** — No prompt/stdin, bad provider, non-embedding provider without `ALCHEMY_RAG_EMBED_PROVIDER`
- [x] **`--output text`** — Answer-only text output format
- [x] **Stdin + prompt combination** — Combines prompt + stdin with `--- stdin ---` separator
- [x] **JSON output format** — Default: `success`, `answer`, `steps`, `tools_used`, `error`
- [x] **AGENTS.md auto-load (pipe)** — Loaded automatically when `--yes` is set

## General

- [x] **Context window management** — Compaction at 85% of `ALCHEMY_CONTEXT_WINDOW`; keeps system prompt + last 6 turns
- [x] **`ALCHEMY_MCP_CONFIG` file loading** — Works alongside `ALCHEMY_MCP_SERVERS` env var
- [x] **TUI log file** — Redirects tracing to `~/.alchemy/debug.log` (or `ALCHEMY_LOG_FILE`)
- [x] **Retry with exponential backoff** — Provider HTTP errors retried up to 3 times with backoff
- [ ] **Cross-platform Windows shell** — `cmd /C` path exists via `#[cfg]` but needs runtime verification on Windows
