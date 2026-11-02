# Contract: page and API additions (010)

Additions to the page API of 007 to 009 (same secret, same origin rules, JSON).

## The project's remote

`GET /api/projects/<id>` gains:

```json
"remote": {
  "state": "known",            // none | no_target | unsupported | known
  "remote": "origin", "service": "github", "repo": "acme/shop", "target": "main",
  "protected": "yes",          // yes | no | unknown | checking
  "why": null,                 // a plain reason for unknown / no_target
  "checked_at": "2026-11-02T20:14:03Z"
}
```

The page shows it next to "git · main": `github · acme/shop · main · protected`, `no remote`,
`protection unknown: <why>`, or `gitlab.com: Fallow merges locally there`.

## The loop

`GET /api/loops/<id>` gains:

```json
"when_verified": "pull_request",          // wait | pull_request
"publish": {
  "offer": ["pull_request"],              // what the page may offer now: pull_request, local_merge
  "why": "main is protected on github.com: changes reach it only through a pull request",
  "remote": { ...as above... },
  "publication": {                        // null until published
    "branch": "fallow/011-order-history/build", "commit": "deec0cf…",
    "pr_number": 42, "pr_url": "https://github.com/acme/shop/pull/42",
    "state": "open", "checks": [{ "name": "test", "state": "passed", "url": "…" }],
    "by": "engineer", "merged_at": null
  },
  "failed": null                          // { step, why } after a failed publication
}
```

Reading the loop asks for the pull request's state if the last was read over 10 s ago (R-6).

`POST /api/loops/<id>/publish` with `{ "confirm": true }`: pushes and opens the pull request
(FR-008, FR-009). Refused, with the reason, when `offer` does not contain `pull_request`, when a
pull request is open (FR-011), or while another action runs. The answer comes before the work is
done; the page follows the loop's events.

`POST /api/loops/<id>/merge` (as today) is refused when `offer` does not contain `local_merge`
(protected target, FR-006).

## The lock

`POST /api/features/<p>/<f>/lock` accepts `when_verified`: `wait` | `pull_request`; when absent,
`pull_request` for the level `free`, `wait` otherwise. The lock form shows it beside Autonomy,
preset by the chosen level, and says "only when the project has a remote".

## Events on the bus

`loop:<id>` as today, for `Published`, `PublishFailed`, `PullRequestState`, `Merged`.
