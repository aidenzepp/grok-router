# grok-router

`grok-router` edits the Grok config that already lives at `$GROK_HOME/config.toml`, or at `~/.grok/config.toml` when `GROK_HOME` is unset. It searches a provider catalog, shows published scores and price from that catalog, inserts one `[model.<slug>]` block, and changes the saved default. It does not switch the model of a running Grok session. `/model` and `grok -m` do that.

The file stores the name of an environment variable in `env_key`. The key value stays in the environment. A write is refused when that value would be copied into the file.

## Install

```bash
cargo install --git https://github.com/aidenzepp/grok-router --locked grok-router
```

From a checkout:

```bash
cargo install --path crates/grok-router --locked
```

## Config

`list`, `search`, `benchmarks`, and `init` read that default path. They do not create it, except `init`, which creates a missing file and any missing parent directory.

`add`, `set-default`, and `remove` require `--config`. They do not open the home file on their own.

```bash
grok-router init
grok-router list
grok-router search glm
grok-router benchmarks
grok-router benchmarks kimi
grok-router add --config ~/.grok/config.toml moonshotai/kimi-k3.1-pro
grok-router set-default --config ~/.grok/config.toml or-kimi-k3-1-pro
grok-router remove --yes --config ~/.grok/config.toml or-kimi-k3-1-pro
```

`init` appends this block when the file has no `[router]` table. An existing `[router]` table is left unchanged. A non-empty file gets one blank line before the new block.

```toml
[router]
default_provider = "openrouter"

[router.provider.openrouter]
base_url = "https://openrouter.ai/api/v1"
env_key = "OPENROUTER_API_KEY"
catalog = "openrouter-models"
```

Another provider is another record. `init` does not write it. The catalog id selects the parser. `openrouter-models` and `deepseek-list-models` are the two catalogs. An unknown id does not fall through to OpenRouter.

```toml
[router.provider.deepseek]
base_url = "https://api.deepseek.com"
env_key = "DEEPSEEK_API_KEY"
catalog = "deepseek-list-models"
```

`--provider <name>` reads that record. Omitting `--provider` uses `default_provider`. Export the named variable where both `grok-router` and Grok can see it. A shell profile or direnv is enough. It does not have to be machine-wide.

## Commands

| Command | Writes | What it does |
| --- | --- | --- |
| `list` | no | Configured models, plus the saved default when that slug has no block. |
| `search` | no | AND-match on id, name, and description. Default `--limit` is 30. |
| `benchmarks` | no | Published price and Artificial Analysis indexes from an `openrouter-models` catalog. Other catalogs refuse before a download. Default `--limit` is 30. |
| `add` | yes | One catalog model, or one direct block. |
| `set-default` | yes | Sets `[models].default` for the next session. |
| `remove` | yes | Deletes one model block. Requires `--yes` or `--force`. |
| `init` | yes | Writes the OpenRouter `[router]` table when the file has none. |
| `skill` | setup and remove | Installs or removes the agent skill stub. |
| `completions` | no | Prints a completion script for bash, zsh, fish, elvish, or powershell. |

`--dry-run` on a write command prints what would change and does not touch the file. `remove` without `--yes` refuses and does not delete. There is no prompt.

`list --limit N` prints the first N rows. Omitting it prints every row. `--limit 0` is refused. `search` and `benchmarks` each keep a limit, and that default is 30.

A catalog is cached for one hour under `~/.cache/grok-router/`. `search` and `benchmarks` share that cache. `--refresh` downloads again. `--catalog-file` reads a local JSON file and skips the network. `--timeout` is seconds for that download, default 30, from `GROK_ROUTER_TIMEOUT` when the flag is omitted. `0` refuses before any request.

## Benchmarks

`benchmarks` reads the catalog `search` already downloads. It does not run prompts and it does not grade answers. With no terms, it prints models that have an Artificial Analysis intelligence index, highest first. Ties stay in catalog order. Terms are the same AND match as `search`, and a match with no score is still shown, in catalog order.

Text columns are the provider id, prompt and completion price in dollars per million tokens, and the intelligence, coding, and agentic indexes. A blank cell is `-`. JSON is an array of those fields plus `name`. Design Arena categories are not in this table.

OpenRouter prices in the document are per token. The table multiplies by one million. A negative price, which OpenRouter uses when a price is not published, prints as `-`.

Only `openrouter-models` has this feed. `deepseek-list-models`, an unknown catalog id, and a provider with no catalog refuse before any download and before `--catalog-file` is read.

## Adding a model

A catalog add resolves one provider, fetches that catalog, and inserts one block. The same slug and the same provider id is a no-op. A different slug for the same provider id exits non-zero and writes nothing. A built-in slug is already in use.

OpenRouter is the only catalog that prefixes a slug. The slug is `or-` plus the last path segment, lowercased, with each run of other characters turned into one hyphen. `moonshotai/kimi-k3.1-pro` becomes `or-kimi-k3-1-pro`. The `model` field keeps the provider id. `--as` replaces that derived slug.

The DeepSeek catalog uses the model id as the slug. A direct add does not derive one. Pass `--as`.

```bash
grok-router add --config ~/.grok/config.toml \
  --as claude-opus \
  --model claude-opus-5-5 \
  --base-url https://proxy.example.com/v1 \
  --name "Claude Opus" \
  --api-backend messages \
  --env-key ANTHROPIC_AUTH_TOKEN
```

On that path all four of `--model`, `--base-url`, `--env-key`, and `--api-backend` are required. A partial set refuses and does not search a catalog. `--name` is optional. `--context-window` and `--vision` are written only when passed. `--default` can be set on either path. It updates `[models].default` in the same write.

Catalog blocks are written in this order: `model`, `base_url`, `name`, `env_key`, `api_backend`, `context_window`, and `vision = true` only when the catalog says the model accepts images. `vision = false` is not written.

## Saved default

`set-default` accepts a built-in slug or a configured slug. When `[router]` has no `builtins` key, the built-ins are `grok-4.7`, `grok-4.7-build-fast`, `grok-4.6`, and `grok-4.5`. `list` does not print those four unless one of them is the saved default and has no block of its own. `remove` refuses a built-in because there is no block to delete.

## Output

`--output text`, `--output json`, and `--output jsonl` are global. `json` and `jsonl` print the same one JSON value. `--json` and `--jsonl` are the same choice without a value. `--quiet` prints one answer line.

`GROK_ROUTER_OUTPUT` and `GROK_ROUTER_QUIET` supply the defaults. A command-line `--output` beats the environment. `--json` or `--jsonl` then beats both. `--json` with `--output text` is an error. `--quiet` with JSON is an error.

JSON errors are one object on stderr: `command`, `format`, `error`, `kind`, and `message`. `error` and `kind` are the same snake_case name. Success JSON stays the command's own value. `list`, `search`, and `benchmarks` succeed with an array.

## Skill stub

`skill setup` writes `<skill-root>/grok-router/SKILL.md`. The stub points an agent at `grok-router __internal skill`. `skill show` prints the guide. `skill remove` deletes that directory only when the stub marker is present and `--yes` is set. A directory without the marker is left in place.

```bash
grok-router skill setup --skill-root ~/.agents/skills
grok-router skill status --skill-root ~/.agents/skills
grok-router skill show
grok-router completions zsh
```

## What a write keeps

A write changes the span it owns. Comments, blank lines, quoting, key order, and unrelated tables stay as they were. The new block is inserted with one blank line around it. A refusal returns the original bytes and says the file was unchanged.

Before a replacement, the previous bytes are copied beside the file as `<file>.prev`. `~/.grok/config.toml` becomes `~/.grok/config.toml.prev`.

## Development

The crate is `crates/grok-router`. From the workspace root:

```bash
cargo fmt
cargo clippy --workspace --all-targets
cargo test --workspace
```
