# Contract: Configuration Files

Location: `<config_dir>/fallow/`, where `config_dir` is:

| Platform | `config_dir` |
|---|---|
| Linux | `$XDG_CONFIG_HOME` if set, non-empty and absolute; otherwise `$HOME/.config` |
| macOS | `$HOME/Library/Application Support` (`XDG_CONFIG_HOME` is ignored) |
| Windows | `%APPDATA%` |

Empty variables count as unset. If no directory can be determined (for example, no `HOME`),
Fallow runs on the built-in defaults only. `--config-dir <DIR>` overrides all of this; the files
are then read from `DIR` itself. The directory is resolved by Fallow's own code from these
variables, with no directories crate. All three files are optional. Built-in defaults are
compiled in, and a user file overrides or extends them (research R-9).

Rules:
- Entries in `signatures.toml` and `providers.toml` are merged by `id`. A user entry with the same
  `id` replaces the built-in entry completely. `enabled = false` disables it.
- Unknown keys are reported as warnings, not errors.
- An invalid value or entry is rejected on its own and reported as `file:line: message`; the
  built-in value or entry is kept. Within one file, a repeated `id` uses the later entry and
  warns about the earlier one.
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

Validation (settings):

| Key | Rule |
|---|---|
| `refresh_interval_ms` | integer, minimum 500 |
| `recent_window_minutes` | integer, minimum 1 |
| `dns.refresh_minutes` | integer, `1..=60` |
| `windows.read_working_directory` | one of `"on_detect"`, `"on_select"`, `"never"` |
| `editor.read_only_threshold_mb` | integer, minimum 1 |
| `editor.tab_width` | integer, `1..=16` |
| `detection.browsers`, `dns.enabled`, `ui.mouse` | boolean |

A value of the wrong type (for example a string where a number belongs) is a parse error: the
whole user file is ignored.

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

Matching:
- **Within one signature, every specified matcher must match (AND).** With both `match.name`
  and `match.cmdline`, a process matches only if its name matches the glob *and* its command line
  matches the regex. So `match.name = "node"` with a Claude Code `match.cmdline` does not match a
  plain `node` process. A signature with `match.cmdline` never matches a process whose command
  line could not be read.
- **Across signatures, any match counts (OR).** A process is detected if at least one enabled
  signature matches it.
- `match.name` is case-insensitive on Windows and macOS, case-sensitive on Linux.

Validation:
- `id` and `kind` are required, plus at least one of `match.name` and `match.cmdline`.
- `kind` is one of the six listed values.
- `local_server` is allowed only with `kind = "local_model_server"`; its `default_ports` are in
  `1..=65535`.
- `match.name` must be a valid glob, and `match.cmdline` and `model_from_cmdline` valid regexes.
  `model_from_cmdline` needs at least one capture group.
- `provider`, if present, must match a provider id after providers are merged.

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
- Hostnames are labels of letters, digits and hyphens (1–63 characters, not starting or ending
  with a hyphen), at most 253 characters in total, with no trailing dot and a non-numeric last
  label. They are stored in lowercase. Ranges are stored with host bits cleared.
- `display_name` defaults to the `id`.

The address ranges shown here are placeholders. The real built-in values are compiled from each
provider's published ranges during implementation and listed with their source in the defaults
file.
