# The project page side rail: behaviour check (T042)

Run on 2026-11-02 against `tests/visual_009.rs` (fake runtime, its own port), with
`node checks/flow.js <url> checks/rail checks/rail-flow.json`. Every step passed; no page errors.
Steps 9b, 9c, 11b, 12b and 12c were added for the review findings (T043). Screenshots in `rail/`.

```text
helpers => "ok"
1 layout: no right column, no Ask the lead to, Features 340 px, rail closed, no badge => {"endColumn":false,"askButtons":0,"leftWidth":340,"rail":true,"open":false,"badge":null,"label":"Proposed features, none"}
2 badge follows the conversation live => {"afterOne":"1","afterThree":"2","label":"Proposed features, 2 not started"}
3 open by its icon: focus on the heading, only unstarted proposals, ends above the message box => {"open":true,"expanded":"true","focus":"side-title","titles":["Receipt by e-mail","Low-stock warning"],"starts":2,"gapToComposer":48}
4 the same icon closes it, focus back on the icon => {"open":false,"expanded":"false","focus":"proposals"}
5 Escape inside the panel closes it, focus back on the icon => {"open":false,"focus":"proposals"}
6 Escape in the message box leaves the panel alone => {"open":true}
7 a click elsewhere closes it and leaves the focus where it went => {"open":false,"focus":"say"}
8 the other icon switches panels => {"open":true,"heading":"What the lead knows","proposalsExpanded":"false","knowsExpanded":"true"}
9 an open panel follows the conversation without closing or losing the focus => {"before":"turns with you2","after":"turns with you3","open":true,"focus":"side-close"}
9b an unchanged panel is not rebuilt: the focused button stays the same element => {"changes":0,"same":true,"open":true}
9c with a panel open, the chat header (Expand) stays reachable => {"expandReachable":true,"panelTopBelowHeader":true}
10 the message box grows: the panel still ends above it => {"gap":12,"panelScrollsInside":true}
11 Where it was proposed: closes, brings the chat's card into view and focuses it => {"open":false,"focused":"Low-stock warning","flash":true,"inView":true}
11b the shown proposal keeps its mark and the focus through a redraw of the chat => {"focused":"Low-stock warning","flash":true,"flashAfter":false}
12 the / menu opens the panels => {"open":true,"heading":"Proposed by the lead · not started","focus":"side-title","box":""}
12b the / menu opens What the lead knows too => {"open":true,"heading":"What the lead knows","focus":"side-title"}
12c a file open in place of the chat: the panel starts under its header, whose buttons stay reachable => {"fileOpen":true,"open":true,"buttons":2,"allReachable":true,"belowHeader":true}
12d back to the chat => {"open":false,"chat":true}
13 Expand keeps the rail; the panel still opens => {"leftShown":false,"rail":"flex","open":true}
narrow => {}
14 narrow window: the panel sits in the page above the chat, no sideways scroll => {"open":true,"position":"static","aboveChat":true,"sideways":false}
wide => {}
15 Start spec session from the panel starts it; back on the project it has left the panel => {"went":"#/p/repo/f/003-receipt-by-e-mail","badge":"1","titles":["Low-stock warning"]}
16 leaving the project closes the panel; it comes back closed => {"open":false,"expanded":"false"}
page errors: none
```
