# Run markdownattractor on a local model

The `local` backend talks to any OpenAI-compatible chat server (llama.cpp, LM Studio, Ollama). It costs nothing per section, works while Claude Code is closed, and raises no policy question because no Anthropic credential is involved. It is slower per call than the API and needs a machine with enough memory for the model.

The reference setup is **llama.cpp** serving **gpt-oss-20b**: Apache-2.0, ~12 GB on disk, runs on a 16–24 GB Mac, and follows JSON schemas reliably, which the card contract needs.

## Setup (three commands)

```bash
# 1. Install llama.cpp (the only dependency besides Homebrew)
brew install llama.cpp

# 2. Download the model and start the server (first run downloads ~12 GB)
llama-server -hf ggml-org/gpt-oss-20b-GGUF \
  --ctx-size 0 \
  --jinja \
  -fa on \
  -b 2048 -ub 2048 \
  -np 4 \
  --port 8080

# 3. Point markdownattractor at it
mda backend local
```

What the flags do:

- `-hf ggml-org/gpt-oss-20b-GGUF` pulls the official build in native MXFP4, the format the model was trained for, so no quality is lost to requantization.
- `--ctx-size 0` uses the model's full native context (128K), split across the `-np` slots. That is the general setting and the right default when memory allows. What markdownattractor actually needs per slot is the largest chunk it sends plus the prompt: chunks are capped at 6K tokens by the planner (`PlanConfig::max_tokens`), so a slot of 16K is safe for every section; `--ctx-size 65536` with `-np 4` gives that and frees several GB for the model weights. If the server logs Metal "command buffer failed" errors or answers HTTP 500 "Compute error", reduce the context or the slots before reaching for a smaller model.
- `--jinja` applies the model's own chat template, which gpt-oss's Harmony format requires.
- `-fa on` turns on flash attention: faster prefill, less memory at long context.
- `-b 2048 -ub 2048` raises the batch sizes, which speeds up prefill. Prefill is the bottleneck when summarizing big sections.
- `-np 4` lets the server hold four requests at once. markdownattractor runs 2 to 4 workers against a local server (never more); decoding several cards at once costs little extra per token on Apple Silicon, so aggregate throughput rises without the per-card latency getting much worse. Use `-np 2` on 16 GB.

The server also serves a chat UI at http://localhost:8080.

## Check it

```bash
curl http://localhost:8080/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "messages": [{"role": "user", "content": "Summarize: markdownattractor indexes markdown."}],
    "chat_template_kwargs": {"reasoning_effort": "low"}
  }'
```

Then `mda doctor` should show the local server and its model, and `mda index` uses it.

`reasoning_effort: low` matters for summarization: at the default setting gpt-oss "thinks" before answering, which delays every card for no gain on this task. markdownattractor sends it per request (`local_reasoning_effort = "low"` in `config.toml`). Recent llama.cpp builds also accept a server-wide default with `--chat-template-kwargs '{"reasoning_effort":"low"}'`.

## Configuration

`.markdownattractor/config.toml`:

```toml
backend = "local"
local_base_url = "http://127.0.0.1:8080/v1"   # include /v1
local_model = "gpt-oss-20b"                    # as the server reports it
local_reasoning_effort = "low"                 # remove the line for servers that reject it
```

LM Studio and Ollama work with the same three lines: set `local_base_url` to their OpenAI-compatible endpoint (`http://localhost:1234/v1` and `http://localhost:11434/v1` by default) and `local_model` to the model name they show. Pick a model that supports JSON-schema output; markdownattractor asks for `response_format: json_schema` with `strict: true` and rejects cards that do not validate.

## Getting the most speed out of it

- Measure your machine: `llama-bench -hf ggml-org/gpt-oss-20b-GGUF -fa 1 -p 8192 -n 128`. The `pp` number is prefill tokens per second, which tells you how long a big section takes to read in.
- On 16 GB, raise the GPU memory cap. macOS limits GPU memory to roughly 65–75% of RAM by default. `sudo sysctl iogpu.wired_limit_mb=13000` raises it and resets on reboot. Close heavy apps (browsers, Docker) first, or the system swaps and everything slows down.
- Plug in power, and avoid long runs on a MacBook Air: low-power mode and thermal throttling cut throughput significantly.
- Keep llama.cpp updated (`brew upgrade llama.cpp`). Metal performance improves often.
- `mda index --limit 50` paces a big backfill. On the local backend the pool starts at 2 workers and never exceeds 4, whatever `concurrency` says, and each call may take up to 5 minutes before it counts as a timeout. Match `-np` on the server to the concurrency you want.
- Memory is the thing to watch: the model must stay resident. If `llama-server`'s resident size is far below the model size and the machine is paging, every card is slow; close other apps or use a smaller model.

## Which model

Measured on an M3 with 24 GB (single request, model fully resident):

| Model | Download | Quality of cards | Decode speed | Notes |
|---|---|---|---|---|
| **gpt-oss-20b** (MXFP4) | 12 GB | best of the three: precise tldrs, dates grounded | ≈ 160 tok/s | needs ~14 GB free; first choice on 24 GB+ |
| **Gemma 3 12B** (Q4_0) | 7 GB | good | not measured yet | the middle option on 16 GB |
| **Gemma 3 4B** (Q4_0) | 2.5 GB | usable: correct tldrs, some vagueness | ≈ 60 tok/s expected | fits anywhere; use when memory is tight |

`-hf ggml-org/gemma-3-4b-it-GGUF` and `-hf ggml-org/gemma-3-12b-it-GGUF` are drop-in replacements in the server command. Any model works as long as the server honours `response_format: json_schema`; the validator rejects cards that do not match the contract, so a weaker model costs retries rather than bad data.

When the same machine was already swapping (26 GB of swap in use from other apps), both gpt-oss-20b and Gemma 3 4B fell to 6–9 tok/s because the weights were being paged in from disk. Free memory first; model size is the second lever.

## When to prefer the API instead

- Corpora of thousands of sections where wall-clock matters: Haiku on the API does about one section per second at 16 workers.
- Machines under 16 GB.
- Highest card quality on tricky sections; Sonnet escalation exists only on the API backend.
