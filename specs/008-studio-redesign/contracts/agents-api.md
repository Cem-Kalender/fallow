# Contract: saving an agent's fields

`PUT /api/agents/<name>?scope=user|project:<p>` (007) accepts either body:

| Body | Meaning |
|---|---|
| `{text}` | The whole definition, as in 007 ("Edit as text") |
| `{fields, body?}` | A map of frontmatter keys to new values, and optionally the instructions; only these lines and the body change, in one write, in the file's own line endings |

`fields` may hold `description`, `runtime`, `model`, `kind`, `may_run` (a list of strings) and `returns`. Any other key is refused (422, naming it). `name` cannot change through `fields`.

The text the change applies to:
1. the definition at that scope, if it exists;
2. otherwise the definition that resolves for the project (or the user), so that "save for this project only" makes a project copy;
3. otherwise a new definition of that name is refused (a new agent is saved with `{text}`).

The rewrite:
- **An existing key:** its line becomes `key: value`, with lists written as `[a, b]`. Values are written so that 007's reader (`agent.rs`) reads them back unchanged: an item or a value that would otherwise read differently (a leading `[`, ` #`, an item holding a space, `,` or `:`) is put in double quotes; a quote may stand only inside a plain value (`[gates, "npm audit"]`); the round trip is a test. The line keeps its position; a trailing comment is kept.
- **An absent key:** it is added as the last line of the frontmatter.
- **Everything else:** other lines, comments, blank lines and the body are kept as they are.

The result then goes through 007's checks, and is refused on any problem, as `{text}` is (422, lines named). That covers a pipeline that would stop parsing or verifying. The reply is `{saved: <path>}`, as in 007.
