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

curl -sS -o /tmp/open-url http://10.0.2.2:8000/open-url && chmod +x /tmp/open-url
stop_safari() {
  for pid in $(ps axo pid,comm | awk '/Safari.app\/Contents\/MacOS\/Safari$/ {print $1}'); do
    kill "$pid" 2>/dev/null
  done
  sleep 3
}

# Safari, attempt 1: a home page preference, set before Safari first runs so no
# cached preferences override it, then a plain launch opens a window on it.
for prefs in /var/root/Library/Preferences \
    /var/root/Library/Containers/com.apple.Safari/Data/Library/Preferences; do
  mkdir -p "$prefs"
  cat > "$prefs/com.apple.Safari.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>HomePage</key><string>$URL/?who=safari-homepage</string>
<key>NewWindowBehavior</key><integer>0</integer>
<key>NewTabBehavior</key><integer>0</integer>
</dict></plist>
PLIST
done
/Applications/Safari.app/Contents/MacOS/Safari > $R/safari-homepage.log 2>&1 &
sleep 45
stop_safari

# Safari, attempt 2: LaunchServices with Safari named as the app for the URL,
# as `open -a Safari <url>` would.
/tmp/open-url -a /Applications/Safari.app/ "$URL/?who=safari-lsopen" > $R/safari-lsopen.log 2>&1
cat $R/safari-lsopen.log
sleep 60

grep '\[viewer\]' $R/server.log
echo PROBE_DONE
