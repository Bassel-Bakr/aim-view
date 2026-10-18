#!/bin/bash
# Packages the Linux server as the release does: aimview-server, ONNX Runtime's WebGPU library, the server-mode UI,
# the models, aimview-server.toml and start-server.sh, into aim-view-server-<version>-linux.tar.gz. Run at the repo's
# root after `bun run build:server` and `cargo build --release -p aimview-server --features webgpu`. Arg: <version>.
set -euo pipefail
dir="aim-view-server-$1-linux"
rm -rf "$dir" && mkdir -p "$dir/ui" "$dir/models"
cp target/release/aimview-server target/release/libwebgpu_dawn.so "$dir/"
cp -r ui/dist/server/browser/. "$dir/ui/"
cp -r ui/generated/models/. "$dir/models/"
cat > "$dir/aimview-server.toml" <<'TOML'
# Aim View's review server: ./aimview-server --help lists every setting. Open http://127.0.0.1:8770/.
ui = 'ui'
models = 'models'
data = 'data'
# the recordings (one folder per scenario), and for other devices on the network a host and a token:
# vods = '/home/me/Videos/KovOBS'
# host = '0.0.0.0'
# token = 'a-long-random-string'
TOML
printf '#!/bin/sh\ncd "$(dirname "$0")" && exec ./aimview-server "$@"\n' > "$dir/start-server.sh"
chmod +x "$dir/start-server.sh" "$dir/aimview-server"
tar -czf "$dir.tar.gz" "$dir"
echo "$dir.tar.gz"
