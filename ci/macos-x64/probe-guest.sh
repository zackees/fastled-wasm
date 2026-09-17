# Feasibility probe for #251: what a docker-mac-x64 Recovery guest can do for
# the viewer and Safari. Runs as root in the Recovery Terminal (bash 3.2).
mkdir -p /tmp/results
R=/tmp/results
section() { echo; echo "=== $* ==="; }
{
section system; date; sw_vers; uname -a; id
section disk; df -h 2>&1; diskutil list 2>&1 | head -40
section tools
for t in python3 curl tar open osascript screencapture safaridriver sqlite3 hdiutil; do
  printf '%s: ' "$t"; command -v "$t" || echo missing
done
section network; curl -sS -o /dev/null -w 'github %{http_code} %{time_total}s\n' --max-time 30 https://github.com/ 2>&1
section webkit
ls -d /System/Library/Frameworks/WebKit.framework 2>&1
find / -maxdepth 5 -name 'Safari.app' -not -path '/Volumes/*' 2>/dev/null | head
find / -maxdepth 5 -name 'safaridriver' -not -path '/Volumes/*' 2>/dev/null | head
section graphics; system_profiler SPDisplaysDataType 2>&1
section processes; ps axo pid,comm | grep -iE 'WindowServer|loginwindow|Dock|Finder|Terminal|Recovery' | head
} > $R/probe.txt 2>&1
cat $R/probe.txt

section "webgl page"
curl -sS -o /tmp/webgl.html http://10.0.2.2:8000/webgl.html
SAFARI=$(find / -maxdepth 5 -name 'Safari.app' -not -path '/Volumes/*' 2>/dev/null | head -1)
echo "safari=$SAFARI"
if [ -n "$SAFARI" ]; then
  open -a "$SAFARI" /tmp/webgl.html 2>&1; echo "open rc=$?"
else
  open /tmp/webgl.html 2>&1; echo "open (default) rc=$?"
fi
sleep 45
screencapture -x $R/screen.png 2>&1; echo "screencapture rc=$?"
ls -l $R
echo PROBE_DONE
