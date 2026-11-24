## macOS (Apple silicon)

**Install with one command** (for you only, no administrator password, no warning):

```sh
curl -fsSL https://raw.githubusercontent.com/Cem-Kalender/fallow/main/installer/macos/install.sh | sh
```

**Or download** `Fallow-@VERSION@-macos-arm64.dmg`, open it and drag Fallow into Applications
(quit a running Fallow first: macOS does not replace an app that is open).

SHA-256: `@SHA256@`

**Not notarized by Apple.** Fallow is signed ad hoc, without Apple's paid developer membership,
so the first time you open a downloaded copy macOS refuses it as coming from an unidentified
developer. To open it once: try to open Fallow, then go to **System Settings → Privacy & Security**,
scroll to *"Fallow" was blocked*, choose **Open Anyway** and confirm. Later launches open without
a warning. (Right-click → Open no longer does this on macOS 15 and later.) The install command
above needs none of this.

**After an upgrade** macOS may ask again for access to Documents, Desktop or Downloads for a
project kept there: each version is a new app to macOS until Fallow is notarized. Notifications
keep their setting.

**Intel Macs**: no download; build Fallow from source (see the README).

**What Fallow cannot switch off**: Fallow's window uses WebKit, which is part of macOS and updated
by Apple with it. Fallow turns off WebKit's fraudulent-website checks and link previews, keeps
nothing of the page on disk, and makes no network request of its own.
@WEBKIT_TRAFFIC@
