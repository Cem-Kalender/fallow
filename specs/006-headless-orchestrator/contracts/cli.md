# Contract: the command line

The command line is thin: it starts or reaches the one service (research R-1) over the user-only
named pipe, and opens pages in the default browser. Exit codes are 0 for success, 1 for an error
(message on stderr), and 2 for bad usage.

| Command | Does | Output |
|---|---|---|
| `fallow` | Starts the service in the background if not running, opens the runs page | The page URL (with the one-time secret) |
| `fallow serve` | Runs the service in the foreground (logs to stderr); refuses if one is already running | Log lines: start, the page URL, runs resumed, "needs you" lines |
| `fallow run "<task>" [--target <branch>] [--autonomy high\|medium\|low] [--limit-usd N] [--limit-min N] [--role <role>=<agent>[:<model>]]...` | In the current repository: checks the setup (FR-032), creates the run, opens its page at the planning conversation | The run id and page URL, or what is missing |
| `fallow status` | Lists runs: id, repository, stage, what needs the engineer | One line per run |
| `fallow stop` | Stops the service. Runs pause and resume at the next start (FR-035) | Confirmation |
| `fallow autostart` | Prints the command that puts a shortcut in the Startup folder. Fallow writes nothing itself | The command |
| `fallow mcp-bridge --step <token>` | Internal: started by an agent step's MCP config (R-5) | MCP over stdio |
| `fallow bench <taskset> [--modes plain,solo,squad] [--repeats 3] [--off <component>]...` | Starts a measured series (FR-029, FR-030) through the service | The series id and page URL |

**Setup check (FR-032)**, run by `fallow run` before anything starts, and shown on the page:
- **the repository:** a git repository with at least one commit, and whether it has uncommitted
  changes, which are left out;
- **the agent:** for each role, the CLI is found, and is signed in. A zero-cost probe is not
  possible, so this check is `claude auth status` or its equivalent per adapter;
- **the gates:** the declared commands exist;
- **the remote:** whether there is one, whether `gh` is signed in, and the target branch's rules;
- **hooks:** the engineer's own Claude Code hooks that will run in every step (R-3), listed for
  information.
