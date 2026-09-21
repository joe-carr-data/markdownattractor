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
  --port 8080

# 3. Point markdownattractor at it
mda backend local
```

What the flags do:

- `-hf ggml-org/gpt-oss-20b-GGUF` pulls the official build in native MXFP4, the format the model was trained for, so no quality is lost to requantization.
- `--ctx-size 0` uses the model's full native context (128K). gpt-oss has a small KV cache, so that fits on 16–24 GB.
- `--jinja` applies the model's own chat template, which gpt-oss's Harmony format requires.
- `-fa on` turns on flash attention: faster prefill, less memory at long context.
- `-b 2048 -ub 2048` raises the batch sizes, which speeds up prefill. Prefill is the bottleneck when summarizing big sections.

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
- `mda index --limit 50` paces a big backfill; `concurrency = 2` in the config keeps the server from queueing too deep. A local server handles one request at a time well and several poorly.

## When to prefer the API instead

- Corpora of thousands of sections where wall-clock matters: Haiku on the API does about one section per second at 16 workers.
- Machines under 16 GB.
- Highest card quality on tricky sections; Sonnet escalation exists only on the API backend.
