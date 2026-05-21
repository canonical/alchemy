# TODO — Missing Features from SPEC.md

## RAG Pipeline

- [x] **Real embedding providers** — OpenAI, Gemini, and Ollama embedders implemented in `embedder.rs`
- [x] **Embedding provider selection** — `RagPipeline::new()` selects based on `ALCHEMY_RAG_EMBED_PROVIDER` / `ALCHEMY_PROVIDER`
- [x] **Validation** — Exit code 2 when RAG enabled with non-embedding provider and no `ALCHEMY_RAG_EMBED_PROVIDER` set
- [x] **Reranking** — `retriever.rs` implements MMR (Maximal Marginal Relevance) reranking. Note: per-chunk embeddings are not persisted, so inter-candidate similarity is approximated via relevance-score proximity rather than true embedding-space distance.
- [ ] **`sqlite-vec` extension** — Spec calls for `sqlite-vec` virtual table (`vec0`); current implementation uses plain SQLite with manual brute-force cosine similarity in Rust
- [ ] **External vector store backends** — Only SQLite implemented. Missing Qdrant and Chroma HTTP backends (`ALCHEMY_RAG_STORE=qdrant|chroma`)

## TUI Mode

- [x] **Panel focus cycling** — `Tab` key cycles focus between panels
- [x] **Scroll support** — `PageUp/PageDown`, `Alt+↑/↓`, and `Ctrl+↑/↓` scrolling implemented; bare `↑/↓` navigates prompt history
- [x] **Typewriter/streaming effect** — Streaming tokens received and accumulated incrementally via `streaming_content`
- [ ] **Shift+Enter for newline** — No `(KeyModifiers::SHIFT, KeyCode::Enter)` arm exists; Shift+Enter is currently a no-op (falls through to the wildcard arm)
- [x] **Ctrl+C interrupt** — Aborts running agent task via `abort_handle`
- [x] **Ctrl+L clear** — Clears conversation messages
- [x] **Ctrl+S manual save** — Saves session to disk
- [x] **Ctrl+D exit** — Exits TUI
- [x] **Syntax highlighting** — `src/tui/markdown.rs` renders assistant messages via pulldown-cmark with styling for headings, bold/italic, inline + fenced code, lists, block quotes, and rules
- [x] **Tool execution spinner animation** — `spinner_frame()` wired up in `layout.rs`; in-progress entries animate while running
- [x] **File activity fade-in animation** — New entries record their tick; renderer fades them yellow → green → default over ~1s
- [x] **Token count real-time updates** — Per-step `StepEvent` channel drives live status-bar updates; authoritative final value on turn end

## Skills System

- [x] **Basic skill loading and trigger matching** — Skill loader, registry, and keyword trigger matching implemented
- [x] **`ALCHEMY_SKILLS_ENABLED=false`** — Skills can be disabled via env var
- [x] **Full SKILL.md body loaded on activation** — `build_skill_context()` reads and appends full body to system prompt
- [x] **Script discovery** — Scripts in `scripts/` dir discovered and registered as skill tools
- [x] **File pattern inference** — `cwd_signals()` derives ecosystem tokens (rust/node/python/go/java/ruby/docker/terraform/github) from marker files and feeds them to `match_skills`
- [x] **Progressive disclosure** — All 3 SPEC stages now staged: metadata at startup (frontmatter only), instructions on activation (`build_skill_context`), resources on-demand (`read_resource` tool + scripts as callable tools)
- [x] **References/assets loading on demand** — Recursively discovered at load time and surfaced via per-skill `read_resource` tool with path-allowlist validation

## MCP Client

- [x] **SSE transport** — Both `stdio` and `sse` transports implemented with full JSON-RPC protocol
- [x] **Graceful degradation logging** — Connection failures logged with `tracing::warn!` and skipped
- [x] **Initialize + notifications/initialized handshake** — Proper MCP protocol initialization on both transports
- [x] **Mid-session crash recovery** — SSE stream errors drain pending oneshots with a descriptive error so in-flight tool calls return failure to the LLM; new calls fail fast via a `closed` atomic flag

## Concourse CI

- [x] **CLI subcommand dispatch** — `main.rs` now exposes `check`/`in`/`out` entrypoints and dispatches to the existing Concourse handlers
- [x] **Metadata completeness** — `in_cmd.rs` and `out.rs` now emit `model` and `duration_secs` in both `metadata.json` and the Concourse `metadata[]` array
- [x] **Out metadata: tools_used missing** — `out.rs` now includes `tools_used` in the Concourse metadata array

## CLI / Pipe Mode

- [x] **`--system` flag** — Implemented as CLI flag
- [x] **`--version` flag** — Implemented via clap `#[command(version)]`
- [x] **Exit code 2 for no prompt/stdin** — Implemented in `run_pipe()`: prints error and returns `Ok(2)`
- [x] **`--output text` in pipe mode** — `output::format_text()` returns answer-only string; verified in code and tests
- [x] **Stdin + prompt combination** — Properly combines prompt + stdin with `--- stdin ---` separator
- [x] **JSON output format** — Default JSON output with `success`, `answer`, `steps`, `tools_used`, `error` fields

## General

- [x] **Context window management** — Compaction strategy implemented in `agent.rs`
- [x] **`ALCHEMY_MCP_CONFIG` file loading** — File-based loading works alongside `ALCHEMY_MCP_SERVERS` env var
- [x] **TUI log file** — TUI mode redirects tracing logs to `~/.alchemy/debug.log` (or `ALCHEMY_LOG_FILE`) to avoid terminal corruption
- [x] **Session persistence** — JSONL-based append-only session storage with load/save in `history.rs`
- [ ] **Cross-platform Windows shell** — `cmd /C` path exists in MCP stdio and builtin `execute_cmd` via `#[cfg]` but needs runtime verification
