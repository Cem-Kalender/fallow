#!/bin/sh
# 016 (contracts/cli-macos.md, research M-21, FR-025 to FR-027, T033): installs Fallow for the
# current user, without an administrator password and without a system warning:
#
#   curl -fsSL https://raw.githubusercontent.com/Cem-Kalender/fallow/main/installer/macos/install.sh | sh
#   curl -fsSL …/install.sh | sh -s -- --version 0.2.0
#   curl -fsSL …/install.sh | sh -s -- --uninstall
#
# It downloads the disk image of a GitHub release and its SHA-256, checks the image against it
# before anything is installed, quits a running Fallow with its Quit confirmation, copies the app
# into ~/Applications and links `fallow` into ~/.local/bin. It uses only what macOS ships (sh,
# curl, shasum, hdiutil, ditto, sysctl, id) and never sudo, spctl, xattr or a system setting:
# a file curl downloads carries no quarantine mark, so macOS has nothing to warn about.
#
# For CI only: FALLOW_INSTALL_FROM=<folder> takes the image and its .sha256 from a folder instead
# of GitHub.
set -eu

REPO=Cem-Kalender/fallow
README=https://github.com/$REPO#build-from-source
APP=$HOME/Applications/Fallow.app
BIN_DIR=$HOME/.local/bin
LINK=$BIN_DIR/fallow
PROFILE=$HOME/.zprofile
MARK="# added by Fallow's installer"

say() { printf '%s\n' "$*"; }
fail() { printf 'fallow install: %s\n' "$*" >&2; exit 1; }

version=
uninstall=
while [ $# -gt 0 ]; do
    case $1 in
        --version)
            [ $# -ge 2 ] || fail "--version needs a version, for example --version 0.2.0"
            version=${2#v}
            shift 2
            ;;
        --uninstall) uninstall=1; shift ;;
        -h | --help)
            say "usage: install.sh [--version <version>] [--uninstall]"
            exit 0
            ;;
        *) fail "unknown option $1 (usage: install.sh [--version <version>] [--uninstall])" ;;
    esac
done

[ "$(id -u)" != 0 ] || fail "run this as yourself, without sudo: Fallow installs for one user"
if [ "$(/usr/sbin/sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" != 1 ]; then
    fail "Fallow's Mac download is for Apple silicon; on an Intel Mac, build it from source: $README"
fi

# Quits a running Fallow with its Quit confirmation (014's `fallow quit --installer`): it waits
# until Fallow has exited. Stops here when the engineer cancels.
quit_fallow() {
    # The installed app's own `fallow` knows `quit`; another `fallow` on PATH (an older build, or
    # another program of that name) may not: for it, an exit other than 0 or 1 means "nothing it
    # can quit", and the install goes on (review).
    exe=$APP/Contents/MacOS/fallow
    strict=1
    if [ ! -x "$exe" ]; then
        exe=$(command -v fallow 2>/dev/null || true)
        strict=
    fi
    [ -n "$exe" ] && [ -x "$exe" ] || return 0
    set +e
    "$exe" quit --installer
    code=$?
    set -e
    case $code in
        0) ;;
        1) fail "Fallow is still running; nothing was changed" ;;
        *)
            if [ -n "$strict" ]; then
                fail "could not quit Fallow (see above); nothing was changed"
            fi
            ;;
    esac
}

if [ -n "$uninstall" ]; then
    quit_fallow
    rm -rf "$APP"
    # Only the link the installer made (to this app), never another file or link there.
    if [ -L "$LINK" ] && [ "$(readlink "$LINK")" = "$APP/Contents/MacOS/fallow" ]; then
        rm -f "$LINK"
    fi
    if [ -f "$PROFILE" ] && grep -qF "$MARK" "$PROFILE"; then
        tmp=$(mktemp)
        # The marked line, and the blank line the installer put before it.
        awk -v mark="$MARK" '
            { line[NR] = $0 }
            END {
                for (i = 1; i <= NR; i++) {
                    if (index(line[i], mark)) continue
                    if (line[i] == "" && i < NR && index(line[i + 1], mark)) continue
                    print line[i]
                }
            }
        ' "$PROFILE" > "$tmp"
        cat "$tmp" > "$PROFILE"
        rm -f "$tmp"
        say "Removed the PATH line Fallow's installer added to ~/.zprofile."
    fi
    say "Fallow is uninstalled: removed $APP and $LINK."
    say "Kept your data: $HOME/Library/Application Support/Fallow (runs, sessions) and $HOME/.fallow (settings). Delete them yourself to remove everything."
    exit 0
fi

work=$(mktemp -d)
mnt=
cleanup() {
    if [ -n "$mnt" ]; then hdiutil detach -quiet "$mnt" 2>/dev/null || true; fi
    rm -rf "$work"
}
trap cleanup EXIT
trap 'cleanup; exit 130' INT TERM

if [ -z "$version" ] && [ -z "${FALLOW_INSTALL_FROM:-}" ]; then
    latest=https://github.com/$REPO/releases/latest
    url=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "$latest") \
        || fail "could not reach $latest"
    version=${url##*/tag/v}
    case $version in
        "" | */* | *:*) fail "no release found at $latest" ;;
    esac
fi
if [ -z "$version" ]; then
    # From a folder (CI): the one image there.
    for f in "$FALLOW_INSTALL_FROM"/Fallow-*-macos-arm64.dmg; do
        [ -f "$f" ] || fail "no Fallow disk image in $FALLOW_INSTALL_FROM"
        v=${f##*/Fallow-}
        version=${v%-macos-arm64.dmg}
    done
fi
case $version in
    *[!0-9A-Za-z.+-]*) fail "not a version: $version" ;;
esac

dmg=Fallow-$version-macos-arm64.dmg
if [ -n "${FALLOW_INSTALL_FROM:-}" ]; then
    for f in "$dmg" "$dmg.sha256"; do
        [ -f "$FALLOW_INSTALL_FROM/$f" ] || fail "Fallow $version: no $f in $FALLOW_INSTALL_FROM"
        cp "$FALLOW_INSTALL_FROM/$f" "$work/$f"
    done
else
    base=https://github.com/$REPO/releases/download/v$version
    for f in "$dmg" "$dmg.sha256"; do
        curl -fsSL -o "$work/$f" "$base/$f" \
            || fail "Fallow $version has no Mac download at $base/$f (releases before 0.2 were for Windows only; see https://github.com/$REPO/releases)"
    done
fi

# The checksum before anything else (FR-025).
if ! (cd "$work" && shasum -a 256 -c "$dmg.sha256" >/dev/null 2>&1); then
    fail "the download does not match its published SHA-256; nothing was installed"
fi

quit_fallow

mnt=$work/mnt
mkdir "$mnt"
hdiutil attach -quiet -nobrowse -readonly -mountpoint "$mnt" "$work/$dmg" \
    || { mnt=; fail "could not open the disk image"; }
[ -d "$mnt/Fallow.app" ] || fail "the disk image holds no Fallow.app"
mkdir -p "$HOME/Applications"
# Copied beside the old app first, then swapped: a failed copy leaves the old one in place.
rm -rf "$APP.new"
ditto "$mnt/Fallow.app" "$APP.new" || { rm -rf "$APP.new"; fail "could not copy Fallow into ~/Applications; nothing was changed"; }
rm -rf "$APP"
mv "$APP.new" "$APP"
hdiutil detach -quiet "$mnt" || true
mnt=

mkdir -p "$BIN_DIR"
linked=
if [ -e "$LINK" ] && [ ! -L "$LINK" ]; then
    say "Left $LINK as it is (a file, not Fallow's link): use $APP/Contents/MacOS/fallow, or remove it and run this again."
else
    ln -sf "$APP/Contents/MacOS/fallow" "$LINK"
    linked=1
fi
added=
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *)
        if ! { [ -f "$PROFILE" ] && grep -qF "$MARK" "$PROFILE"; }; then
            # shellcheck disable=SC2016 # written as it is, for the shell to expand later
            printf '\nexport PATH="$HOME/.local/bin:$PATH" %s\n' "$MARK" >> "$PROFILE"
            added=1
        fi
        ;;
esac

say "Installed Fallow $version in $APP."
if [ -n "$linked" ]; then
    say "Linked fallow: $LINK"
fi
if [ -n "$added" ]; then
    say "Added ~/.local/bin to your PATH in ~/.zprofile (new terminals have it)."
fi
say "Open Fallow from Applications or type \`fallow\`."
