# Alchemy

A cross-platform CI/CD AI agent written in Rust. Single static binary, dual-mode: **pipe mode** for automation, **TUI mode** for interactive use.

See [SPEC.md](SPEC.md) for the full design specification. This README documents what is currently implemented.

## Status

Working in v0.1.0:

- Pipe mode (`alchemy "prompt"` or `echo prompt | alchemy`)
- Interactive TUI mode (`alchemy tui`)
- RAG indexing and search CLI (`alchemy rag …`)
- Six LLM providers
- Five built-in tools
- MCP client (stdio + SSE) — pipe mode
- Skills system (Agent Skills Specification) — pipe mode
- Concourse CI `check` / `in` / `out` lifecycle
- `sqlite-vec` virtual table for RAG — the current store falls back to a full-scan cosine similarity in Rust; functional but unindexed
- MCP / skills / RAG inside TUI mode

## Build

```bash
cargo build --release          # → target/release/alchemy
cargo test                     # unit + integration tests
ALCHEMY_E2E=1 cargo test       # also run E2E (requires a real API key)
cargo clippy --all-targets -- -D warnings
```

## Quick start

```bash
export ALCHEMY_PROVIDER=openai
export ALCHEMY_API_KEY=sk-...

# One-shot prompt
alchemy "List the files in src/"

# Stdin pipe
git diff HEAD~1 | alchemy --output text "Summarize this diff"

# Both: prompt is the instruction, stdin is appended as an input block
cargo test 2>&1 | alchemy "Analyze the failures and suggest fixes"

# Interactive
alchemy tui
```

Run TUI directly from the published container image:

```bash
# OpenRouter
docker run --env ALCHEMY_PROVIDER=openrouter --env ALCHEMY_API_KEY=sk-or-v1-... --env ALCHEMY_MODEL=google/gemma-4-31b-it --rm -it ghcr.io/canonical/alchemy:latest tui

# GitHub Copilot
docker run --env ALCHEMY_PROVIDER=github-copilot --env ALCHEMY_API_KEY=ghu_... --env ALCHEMY_MODEL=claude-sonnet-4.6 --rm -it ghcr.io/canonical/alchemy:latest tui

# Gemini
docker run --env ALCHEMY_PROVIDER=gemini --env ALCHEMY_API_KEY=AI... --env ALCHEMY_MODEL=gemini-3.1-flash-lite-preview --rm -it ghcr.io/canonical/alchemy:latest tui
```

## Providers

| `ALCHEMY_PROVIDER` | API key | Default `ALCHEMY_MODEL` |
|---|---|---|
| `openai` | required | `gpt-4o-mini` |
| `openrouter` | required | `gpt-4o-mini` |
| `github-copilot` | required (PAT) | `gpt-5-mini` |
| `gemini` | required | `gemini-2.0-flash` |
| `anthropic` | required | `claude-3-5-haiku-latest` |
| `ollama` | optional | `llama3.2` |

`ALCHEMY_BASE_URL` overrides the default endpoint per provider.

## Built-in tools

| Tool | Purpose |
|---|---|
| `read_file` | Read a UTF-8 file (truncated at 32KB) |
| `write_file` | Write a file, creating parent directories |
| `list_dir` | List directory entries |
| `execute_cmd` | Run a shell command (`sh -c` / `cmd /C`) with timeout |
| `fetch_url` | HTTP GET with timeout (truncated at 32KB) |

MCP tools are exposed as `mcp_<server>_<tool>`; skill scripts as `skill_<skill>_<script>`.

`read_file`, `list_dir`, and `fetch_url` may execute concurrently inside a single LLM turn; `write_file`, `execute_cmd`, MCP, and skill calls run sequentially.

## Configuration

All knobs are environment variables; CLI flags override env vars.

### Core

| Variable | Default | Description |
|---|---|---|
| `ALCHEMY_PROVIDER` | — | **Required.** One of the providers above |
| `ALCHEMY_API_KEY` | — | Required for all non-ollama providers |
| `ALCHEMY_MODEL` | provider default | Model name |
| `ALCHEMY_BASE_URL` | per-provider | Custom endpoint |
| `ALCHEMY_SYSTEM_PROMPT` | built-in | Custom system prompt |
| `ALCHEMY_MAX_STEPS` | `30` | Agent loop iteration limit |
| `ALCHEMY_TIMEOUT_SECS` | `30` | Per-tool timeout (`execute_cmd`, `fetch_url`) |
| `ALCHEMY_CONTEXT_WINDOW` | `128000` | Drives context compaction at 85% |
| `ALCHEMY_OUTPUT` | `json` | `json` or `text` (pipe mode) |
| `ALCHEMY_LOG_LEVEL` | `warn` | `error` / `warn` / `info` / `debug` / `trace` |
| `ALCHEMY_LOG_FILE` | `~/.alchemy/debug.log` | TUI log destination |

### Extensions

| Variable | Default | Description |
|---|---|---|
| `ALCHEMY_MCP_SERVERS` | — | Inline JSON MCP server list |
| `ALCHEMY_MCP_CONFIG` | `~/.alchemy/mcp.json` | MCP config file |
| `ALCHEMY_SKILLS_ENABLED` | `true` | Toggle skills system |
| `ALCHEMY_SKILLS_DIR` | `~/.alchemy/skills` | Skills directory |
| `ALCHEMY_SESSION_DIR` | `~/.alchemy/sessions` | TUI session storage |

### RAG

`ALCHEMY_RAG_ENABLED=true` enables RAG in pipe mode. The embedding provider must support embeddings (`openai`, `gemini`, or `ollama`); otherwise set `ALCHEMY_RAG_EMBED_PROVIDER` explicitly or Alchemy exits with code 2.

| Variable | Default |
|---|---|
| `ALCHEMY_RAG_ENABLED` | `false` |
| `ALCHEMY_RAG_EMBED_PROVIDER` | same as `ALCHEMY_PROVIDER` |
| `ALCHEMY_RAG_EMBED_MODEL` | provider default |
| `ALCHEMY_RAG_STORE_PATH` | `~/.alchemy/rag/vectors.db` |
| `ALCHEMY_RAG_CHUNK_SIZE` | `512` |
| `ALCHEMY_RAG_CHUNK_OVERLAP` | `64` |
| `ALCHEMY_RAG_TOP_K` | `5` |

```bash
alchemy rag index src/
alchemy rag search "agent loop"
alchemy rag status
alchemy rag clear
```

## Concourse CI

The published container image is `ghcr.io/canonical/alchemy:latest`. It ships `/opt/resource/check`, `/opt/resource/in`, and `/opt/resource/out` wrappers for use as a Concourse resource type, and it can also be used directly as a task image.

### Resource type

```yaml
resource_types:
  - name: alchemy
    type: registry-image
    check_every: 24h
    source:
      repository: ghcr.io/canonical/alchemy
      tag: latest

resources:
  - name: weather
    type: alchemy
    icon: weather-partly-cloudy
    check_every: 1h
    source:
      provider: github-copilot
      api_key: ((ai-provider/github-copilot.api-key))
      model: gpt-5-mini
      prompt: "Fetch the weather for Madrid from wttr.in using curl."
  - name: alchemy
    type: registry-image
    icon: si/alchemy
    check_every: 24h
    source:
      repository: ghcr.io/canonical/alchemy
      tag: latest

jobs:
  - name: check-weather
    public: true
    plan:
      - get: weather
        trigger: true
      - get: alchemy
      - task: check
        image: alchemy
        config:
          platform: linux
          inputs:
            - name: weather
          run:
            path: sh
            args:
              - -c
              - |
                cat weather/response.txt
```

### Task image

```yaml
jobs:
  - name: review-diff
    plan:
      - get: source-code
        trigger: true
      - task: summarize
        config:
          platform: linux
          image_resource:
            type: registry-image
            source:
              repository: ghcr.io/canonical/alchemy
              tag: latest
          inputs:
            - name: source-code
          run:
            path: sh
            args:
              - -c
              - |
                cd source-code
                git diff HEAD~1 | alchemy --output text "Review this diff for bugs and security issues"
```

## CLI

```text
alchemy [OPTIONS] [PROMPT]                              # Pipe mode (default)
alchemy tui [OPTIONS]                                   # Interactive TUI
alchemy rag {index <PATH> [--glob G] | search <Q> | status | clear}
```

### Pipe mode options

```
--output <json|text>   Output format (default: json)
--system <PROMPT>      System prompt override
--max-steps <N>        Override ALCHEMY_MAX_STEPS
--timeout <SECS>       Override ALCHEMY_TIMEOUT_SECS
```

### Output

JSON (default):

```json
{
  "success": true,
  "answer": "All 42 tests passed.",
  "steps": 3,
  "tools_used": ["execute_cmd", "read_file"],
  "error": null
}
```

Text mode prints just the final answer.

### Exit codes

| Code | Meaning |
|---|---|
| 0 | Agent produced a final answer |
| 1 | Agent/runtime failure (provider error after retries, max steps exceeded, …) |
| 2 | Configuration error (missing provider/key, no prompt or stdin, bad input, …) |

## Architecture

```
src/
├── main.rs          entry point and subcommand dispatch
├── agent.rs         agent loop (LLM call → tool dispatch → repeat)
├── providers/       openai · openrouter · ollama · github-copilot · gemini · anthropic
├── tools/           built-in + MCP + skill tool registry
├── skills/          Agent Skills Specification loader
├── rag/             chunker · embedder · store · retriever
├── tui/             ratatui + crossterm interactive UI
├── concourse/       Concourse CI lifecycle
├── types.rs         shared message / config types
└── output.rs        pipe-mode output formatting
```

The agent core is frontend-agnostic — pipe mode and TUI mode are adapters over the same loop in `src/agent.rs`.

See [CLAUDE.md](CLAUDE.md) for development guidance and [SPEC.md](SPEC.md) for the full specification.
