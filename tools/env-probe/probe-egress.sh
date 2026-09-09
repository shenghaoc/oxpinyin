#!/usr/bin/env bash
# Can this host reach what the perf and oracle images need?
#
# The perf-matrix and callgrind images are not self-contained: they fetch
# a base image, Debian packages, rustup, crates and the pinned libpinyin
# tree at build time. On a network with an egress allowlist any one of
# those can be denied, and the failure surfaces deep inside a long
# `docker build` as an opaque 403. Run this first.
#
#   bash tools/env-probe/probe-egress.sh
#
# Exit 0 if every host needed for a perf image build is reachable.
#
# Measured on the Claude Code remote environment, 2026-09-08 UTC: the
# Debian repositories and Docker Hub's blob CDN were denied, which blocks
# libtkrzw-dev and so blocks the libpinyin cell entirely. github.com,
# static.rust-lang.org, the crates registry and mirror.gcr.io were
# reachable. See the handoff on
# perf/steady-cycle-constant-factor-mkeg4k.
#
# A CONNECT denial hides its body from curl, so where an agent proxy
# exposes a status endpoint this also prints the reason it recorded.

set -uo pipefail

# url<TAB>label<TAB>why-it-is-needed
#
# Probe a real path, never a bare host root: several of these answer 400
# or 403 at "/" while serving their actual content perfectly. Treating
# that as a denial sends the next reader chasing a phantom.
PROBES=(
    "https://deb.debian.org/debian/dists/stable/Release	deb.debian.org	Debian packages (libtkrzw-dev, libkyotocabinet-dev, glib, valgrind)"
    "https://snapshot.debian.org/archive/debian/20260831T000000Z/dists/testing/Release	snapshot.debian.org	the pinned apt snapshot the perf images use"
    "https://production.cloudfront.docker.com/	cloudfront.docker.com	Docker Hub blob CDN (base image layers)"
    "https://registry-1.docker.io/v2/	registry-1.docker.io	Docker Hub manifests"
    "https://mirror.gcr.io/v2/	mirror.gcr.io	a mirror that can serve the same base image digest"
    "https://static.rust-lang.org/rustup/archive/1.29.0/x86_64-unknown-linux-gnu/rustup-init.sha256	static.rust-lang.org	rustup-init"
    "https://index.crates.io/config.json	index.crates.io	the cargo registry index"
    "https://static.crates.io/crates/libc/libc-0.2.189.crate	static.crates.io	crate downloads"
    "https://github.com/libpinyin/libpinyin.git/info/refs?service=git-upload-pack	github.com	the pinned libpinyin and ibus-libpinyin trees"
)

printf '%-24s %-6s %-8s %s\n' HOST CODE VERDICT NEEDED-FOR
printf '%-24s %-6s %-8s %s\n' "-----------------------" "-----" "-------" "----------"

failed=0
for entry in "${PROBES[@]}"; do
    url=$(printf '%s' "$entry" | cut -f1)
    host=$(printf '%s' "$entry" | cut -f2)
    need=$(printf '%s' "$entry" | cut -f3)
    code=$(curl -s -o /dev/null -w '%{http_code}' --max-time 20 "$url" 2>/dev/null)
    # Denial looks like exactly three things. 000 is "no HTTP response at
    # all" — DNS failure, refused, or a CONNECT rejected before any status
    # line. 403/407 are the proxy refusing. A 401 (registries) or a 3xx
    # (mirrors) means the host answered, which is all this asks.
    verdict=ok
    case "$code" in
        000|403|407) verdict=DENIED; failed=1 ;;
    esac
    printf '%-24s %-6s %-8s %s\n' "$host" "$code" "$verdict" "$need"
done

# An agent proxy records why it rejected a CONNECT; curl cannot show it.
if [ -n "${HTTPS_PROXY:-}" ]; then
    echo
    echo "--- proxy-recorded denials (deduplicated) ---"
    curl -sS "${HTTPS_PROXY}/__agentproxy/status" 2>/dev/null | python3 -c '
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    sys.exit(0)
seen = set()
for f in d.get("recentRelayFailures", []):
    host = f.get("host")
    if not host:
        continue
    key = (host, f.get("kind"))
    if key in seen:
        continue
    seen.add(key)
    print(f'"'"'  {host:<42} {f.get("kind")}: {f.get("detail")}'"'"')
' || true
fi

echo
if [ "$failed" -ne 0 ]; then
    echo "RESULT: at least one required host is unreachable or denied."
    echo "        A perf image build will fail. Report the blocked host;"
    echo "        do not route around an egress policy."
    exit 1
fi
echo "RESULT: every host a perf image build needs is reachable."
