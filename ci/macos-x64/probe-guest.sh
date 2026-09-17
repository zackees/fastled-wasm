# Feasibility probe for #251, round 2: can WebKit in a Recovery guest give the
# shipped viewer and Safari a WebGL2 context and animation frames?
mkdir -p /tmp/results /tmp/page /tmp/home
R=/tmp/results
# Recovery's /var/root is read-only; fastled needs a writable ~/.fastled.
export HOME=/tmp/home
cd /tmp
curl -sS -o /tmp/fastled http://10.0.2.2:8000/fastled && chmod +x /tmp/fastled
curl -sS -o /tmp/page/index.html http://10.0.2.2:8000/webgl.html
/tmp/fastled --version 2>&1; echo "fastled --version rc=$?"

/tmp/fastled --internal-serve-dir-headless /tmp/page > $R/server.log 2>&1 &
URL=
for i in $(seq 60); do
  URL=$(grep -Eo 'http://127\.0\.0\.1:[0-9]+' $R/server.log | head -1)
  [ -n "$URL" ] && break
  sleep 1
done
echo "url=$URL"
cat $R/server.log

echo "--- shipped viewer"
/tmp/fastled --internal-viewer "$URL/?who=viewer" > $R/viewer.log 2>&1 &
VIEWER=$!
sleep 45
echo "viewer alive: $(kill -0 $VIEWER 2>/dev/null && echo yes || echo no)"
cat $R/viewer.log | tail -40

echo "--- safari"
/Applications/Safari.app/Contents/MacOS/Safari "$URL/?who=safari" > $R/safari.log 2>&1 &
SAFARI=$!
sleep 45
echo "safari alive: $(kill -0 $SAFARI 2>/dev/null && echo yes || echo no)"
tail -40 $R/safari.log

echo "--- reports"
grep '\[viewer\]' $R/server.log
ps axo pid,comm | grep -iE 'safari|webkit|fastled|WebContent|GPU' | head -20
echo PROBE_DONE
