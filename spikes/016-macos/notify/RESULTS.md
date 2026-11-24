# Spike 016: notifications from an ad-hoc signed app (research M-9)

Throwaway. A minimal app (`main.swift`, `Info.plist`) that asks `UNUserNotificationCenter` for
authorization and posts one notification, signed ad hoc (`codesign --force --deep -s -`), with
no Apple certificate.

Its `main.swift` and `Info.plist` moved to `ci/macos/notify-check/` (016 T044), where CI runs
them on each supported macOS version (`check.sh`). Build and run, from there (writes its result
to the file given):

```sh
mkdir -p FallowNotifySpike.app/Contents/MacOS
cp Info.plist FallowNotifySpike.app/Contents/
swiftc -O main.swift -o FallowNotifySpike.app/Contents/MacOS/spike
codesign --force --deep -s - FallowNotifySpike.app
cp -R FallowNotifySpike.app ~/Applications/
: > result.txt   # the app writes into an existing file only
open -W ~/Applications/FallowNotifySpike.app --args "$PWD/result.txt"; cat result.txt
```

`main.swift` takes a second argument `provisional` to ask for provisional authorization.

## Results

2026-11-22, the engineer's Mac (macOS 26.7.3, Apple silicon, managed: MDM and an endpoint agent).

| Run | Where | Asked | Result |
|---|---|---|---|
| 1 | a temporary folder, accessory app | alert, sound | Refused at once: `UNErrorDomain` 1, "Notifications are not allowed for this application"; the app was not registered with LaunchServices. |
| 2 | `~/Applications`, regular app, `dev.fallow.notifyspike2` | alert, sound | No answer within 120 s (a prompt nobody was there to click). |
| 3 | the same bundle id, later | alert, sound, provisional | Status already `denied` (1) before asking: the unanswered prompt ended as a refusal. |
| 4 | `~/Applications`, regular app, new id `dev.fallow.notifyspike3` | alert, sound, provisional | **`granted=true`, status provisional (3), no prompt; delivered=1.** usernoted: "Authorization set for dev.fallow.notifyspike3 to allow: YES … provisionally: YES". |
| 5 | run 4's app rebuilt and signed again (new code hash) | the same | **Status provisional before asking (kept), delivered=2.** |

## Conclusion

An ad-hoc signed app can notify: provisional authorization needs no prompt and no certificate, and
macOS keeps it across new signatures (it is keyed on the bundle id). Fallow asks provisionally at
the first notification and offers full banners from its setting (research M-9). Left for the tasks,
with someone at the Mac: a click on a notification reaching the app, and the full-authorization
prompt.

Test apps removed from `~/Applications` after the runs.
