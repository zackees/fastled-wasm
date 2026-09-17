# Runs as root in a docker-mac-x64 Recovery guest (bash 3.2, no python, no
# `open`, no safaridriver). Loads ci/macos-x64/webgl.html in the shipped viewer
# (`fastled --internal-viewer`, WKWebView) and in Safari, and records what each
# reports through the fastled server's /viewer-log route. See
# .github/workflows/macos-x64-guest-webkit-probe.yml for why this exists.
mkdir -p /tmp/results /tmp/page /tmp/home
R=/tmp/results
# Recovery's /var/root is read-only; fastled needs a writable ~/.fastled.
export HOME=/tmp/home
cd /tmp
{
  sw_vers
  uname -m
} > $R/guest.txt 2>&1
curl -sS -o /tmp/fastled http://10.0.2.2:8000/fastled && chmod +x /tmp/fastled
curl -sS -o /tmp/page/index.html http://10.0.2.2:8000/webgl.html
/tmp/fastled --version >> $R/guest.txt 2>&1

/tmp/fastled --internal-serve-dir-headless /tmp/page > $R/server.log 2>&1 &
URL=
for i in $(seq 60); do
  URL=$(grep -Eo 'http://127\.0\.0\.1:[0-9]+' $R/server.log | head -1)
  [ -n "$URL" ] && break
  sleep 1
done
echo "url=$URL"

# The shipped viewer: the same WKWebView window `fastled <sketch>` opens.
/tmp/fastled --internal-viewer "$URL/?who=viewer" > $R/viewer.log 2>&1 &
sleep 45
kill $! 2>/dev/null

# Safari: LaunchServices opens the URL, as `open` would. Recovery has no
# `open`, and Safari reads a command-line argument as a sandboxed file path.
curl -sS -o /tmp/open-url http://10.0.2.2:8000/open-url && chmod +x /tmp/open-url
/tmp/open-url "$URL/?who=safari" > $R/safari.log 2>&1
sleep 60

grep '\[viewer\]' $R/server.log
echo PROBE_DONE
