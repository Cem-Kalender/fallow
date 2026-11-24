#!/bin/sh
# 016 (SC-007, research M-7, M-16, T043): the release app opened through LaunchServices, its
# window idle for 10 minutes, every packet captured with the process that sent or received it
# (`tcpdump -i pktap,all -k NP`, which needs sudo: GitHub's macOS runners allow it).
# - Fallow's own process opens no connection: the job fails otherwise.
# - WebKit's processes' connections, and the names the machine looked up meanwhile, are listed
#   in the job's summary (constitution V, FR-017: what WebKit does that Fallow cannot switch off).
# - Once installer/macos/webkit-traffic.md lists hosts (one per line, `- host`), a looked-up
#   name not on it is flagged in the summary for the engineer to review.
#
#   sh ci/macos/trace.sh <Fallow.app> [minutes]
set -eu
# Absolute: `open -a` takes a relative path for an app's name.
app=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
minutes=${2:-10}
work=$(mktemp -d)
summary=${GITHUB_STEP_SUMMARY:-/dev/stdout}
exe=$app/Contents/MacOS/fallow

sudo tcpdump -i pktap,all -k NP -n -w "$work/trace.pcapng" >/dev/null 2>&1 &
sleep 3
pgrep -x tcpdump >/dev/null || { echo "trace: tcpdump did not start"; exit 1; }
open -a "$app"
end=$(( $(date +%s) + 30 ))
until "$exe" status >/dev/null 2>&1; do
    [ "$(date +%s)" -lt "$end" ] || { echo "trace: Fallow did not start"; sudo pkill -INT -x tcpdump; exit 1; }
    sleep 1
done
echo "trace: Fallow is running with its window; idle for $minutes minutes"
sleep $(( minutes * 60 ))
"$exe" stop || true
sleep 3
# tcpdump itself, not the sudo that started it (sudo does not pass on a signal from its own
# process group); then waited for, at most 30 s, so the file is complete.
sudo pkill -INT -x tcpdump || true
n=0
while pgrep -x tcpdump >/dev/null && [ "$n" -lt 30 ]; do sleep 1; n=$((n + 1)); done
sudo chown "$(id -u)" "$work/trace.pcapng"
tcpdump -n -r "$work/trace.pcapng" -k NP > "$work/trace.txt"

python3 -I - "$work/trace.txt" "installer/macos/webkit-traffic.md" "$summary" <<'PY'
import ipaddress, re, sys
text = open(sys.argv[1], errors="replace").read().splitlines()
# The hosts listed in webkit-traffic.md, its comments left out (none listed yet: no flags).
listed = re.sub(r"<!--.*?-->", "", open(sys.argv[2]).read(), flags=re.S)
# Hosts are listed one per line as `- host`.
hosts = set(re.findall(r"^- (\S+)", listed, re.M))
out = open(sys.argv[3], "a")
by_proc = {}
names = set()
attributed = 0
proc_re = re.compile(r"\bproc ([^:,()]+):(\d+)")
v4 = re.compile(r"\b(\d+\.\d+\.\d+\.\d+)\.(\d+)\b")
v6 = re.compile(r"\b([0-9a-fA-F]*:[0-9a-fA-F:]+)\.(\d+)\b")
def remote(addr):
    try:
        ip = ipaddress.ip_address(addr)
    except ValueError:
        return False
    return not (ip.is_private or ip.is_loopback or ip.is_link_local or ip.is_multicast
                or ip.is_unspecified)
for line in text:
    m = proc_re.search(line)
    if m:
        attributed += 1
    proc = m.group(1) if m else "?"
    for a, port in v4.findall(line) + v6.findall(line):
        if remote(a):
            by_proc.setdefault(proc, set()).add(f"{a}:{port}")
    for q in re.findall(r" (?:A|AAAA|HTTPS)\? ([A-Za-z0-9.-]+)\.", line):
        names.add(q.lower())
print("## Network trace (macOS, window idle)", file=out)
print(f"\nPackets seen: {len(text)}, with their process: {attributed}", file=out)
# A positive control: the capture saw packets and named their processes; otherwise the gate
# would pass on nothing (review).
if not text or attributed == 0:
    print("\n**FAIL**: no packet with its process was captured", file=out)
    print("trace: no packet with its process was captured (is the pktap format as parsed?)")
    sys.exit(1)
own = sorted(by_proc.get("fallow", set()))
for proc in sorted(by_proc):
    if proc == "fallow" or proc.startswith("com.apple.WebKit"):
        print(f"\n**{proc}**: " + (", ".join(sorted(by_proc[proc])) or "none"), file=out)
print("\n**Names looked up on the machine meanwhile**:", file=out)
for n in sorted(names):
    flag = "" if (not hosts or n in hosts) else "  ← not in webkit-traffic.md: review"
    print(f"- {n}{flag}", file=out)
if own:
    print(f"\n**FAIL**: Fallow's own process connected: {own}", file=out)
    print(f"trace: Fallow's own process connected: {own}")
    sys.exit(1)
print("trace: no connection of Fallow's own")
PY
# The summary page is not readable through GitHub's API; the log is, for webkit-traffic.md.
[ "$summary" = /dev/stdout ] || cat "$summary"
