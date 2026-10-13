# Contract: Configuration Files

Location: `<config_dir>/fallow/`, where `config_dir` is `$XDG_CONFIG_HOME` or `~/.config` on Linux,
`~/Library/Application Support` on macOS, and `%APPDATA%` on Windows. It can be overridden by
`--config-dir`. All three files are optional. Built-in defaults are compiled in, and a user
file overrides or extends them (research R-9).

Rules:
- Entries in `signatures.toml` and `providers.toml` are merged by `id`. A user entry with the same
  `id` replaces the built-in entry completely. `enabled = false` disables it.
- Unknown keys are reported as warnings, not errors.
- If a file fails to parse, that file's user entries are ignored, the defaults remain, and the UI
  shows `file:line: message`.
- Files are reloaded only by the "Reload configuration" command.

## settings.toml

```toml
refresh_interval_ms = 2000          # FR-015; minimum 500
recent_window_minutes = 30          # FR-017

[detection]
browsers = false                    # FR-026

[dns]
enabled = true                      # FR-022b
refresh_minutes = 15                # research R-5; 1..=60

[windows]
read_working_directory = "on_detect" # "on_detect" | "on_select" | "never"   (research R-10)

[editor]
read_only_threshold_mb = 10
tab_width = 4

[ui]
mouse = true                        # FR-063
```

## signatures.toml

```toml
[[signature]]
id = "claude-code"
kind = "agent_cli"                  # agent_cli | script | editor | local_model_server | unknown | browser
provider = "anthropic"              # optional, must match a provider id
match.name = "claude*"              # optional glob on the process name
match.cmdline = '(^|[\\/ ])claude(\.exe)?( |$)'   # optional regex on argv joined with spaces

[[signature]]
id = "ollama"
kind = "local_model_server"
match.name = "ollama*"
model_from_cmdline = 'run\s+(\S+)'  # optional; first capture group is the model name
local_server.default_ports = [11434]

[[signature]]
id = "chrome"
kind = "browser"
match.name = "chrome*"
enabled = true                      # browsers are still excluded unless detection.browsers = true
```

Validation: `id` and `kind` are required, plus at least one of `match.name` and `match.cmdline`.
`local_server` is allowed only with `kind = "local_model_server"`.

The concrete built-in list is curated during implementation and versioned with the binary.

## providers.toml

```toml
[[provider]]
id = "anthropic"
display_name = "Anthropic"
hostnames = ["api.anthropic.com"]   # the ONLY names Fallow may resolve (FR-022a)
ranges = ["160.79.104.0/23"]        # built-in fallback (FR-022b); illustrative value

[[provider]]
id = "openai"
display_name = "OpenAI"
hostnames = ["api.openai.com"]
ranges = []
```

Validation:
- `hostnames` entries must be plain DNS names, with no wildcards and no IP literals.
- `ranges` entries must be valid CIDR.
- A provider with neither hostnames nor ranges is rejected.

The address ranges shown here are placeholders. The real built-in values are compiled from each
provider's published ranges during implementation and listed with their source in the defaults
file.
