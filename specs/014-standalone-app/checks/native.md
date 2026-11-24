# Native behaviour (T023), 2026-11-14

Checked on the engineer's laptop, debug build, without the engineer at the keyboard:

- the window opens on the dashboard, titled Fallow with its icon, in 013's dark theme from the
  first frame; closing it ends the web view's processes and keeps the service; its place is kept
  and comes back the same (`window.json` unchanged across three closes and opens);
- `fallow` while it is open brings it forward; `fallow quit` with nothing running quits at once.

Waiting for the engineer (they need clicks, drags and keys; quickstart Q3, T034):

- the folder picker in front of the window, as its dialog;
- an image attached by the file dialog and by dragging it onto a session's message box; a file
  dropped anywhere else does nothing; an attached image opens in the page's viewer;
- copy, cut, paste, select all, undo, redo, zoom; reload keeping the route; the context menu
  without "open link in new window", "save as", "print", "inspect" (release build);
- a notification's click with the window closed opening it on the item;
- Quit with a loop running: the dialog names it; Cancel keeps everything.
