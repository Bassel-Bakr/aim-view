# aimview-server

Aim View's review server, in Rust. It serves the UI's server-mode build and
the review server's API, the one the Python server served (`python/retired/`). The review runs natively, as in the
desktop app: both use the `aimview-service` crate (`service/`). It is the server the UI's server mode talks to.

## Run it

```bash
bun run build:server     # the UI's server-mode build, into ui/dist/server/browser (again after UI changes)
bun run server           # cargo run -p aimview-server --release
```

Then open http://127.0.0.1:8770/. Ctrl+C stops the server.

`bun run dev:server` (the Angular dev server) sends `/api` and `/video` to port 8770, so it works with this server
as it did with Python's. The server's own UI build is then not needed.

## Settings

With no settings, the server runs on the machine Aim View is made on:

| Setting | Flag | Default |
| --- | --- | --- |
| Address | `--host` | `127.0.0.1` |
| Port | `--port` | `8770` |
| Data folder | `--data` | the repo's `test_out/`, in the Python server's layout |
| Recordings | `--vods` | `E:\OBS\KovOBS` (`--vods=` for the folder last chosen in the app) |
| KovaaK's stats files | `--stats` | `...\steamapps\common\FPSAimTrainer\FPSAimTrainer\stats` |
| Scenario folders | `--scenarios` (several) | KovaaK's `Saved\SaveGames\Scenarios` and `steamapps\workshop\content\824270` |
| Models | `--models` | the repo's `python/model/exports` (`models.json` is found in the folder above) |
| Device | `--device` | `auto`; also `directml`, `cuda`, `cpu` |
| GPU frames | `--gpu-frames` | `on`: decoded and converted on the GPU where the video allows it (Windows, 2560 x 1440 AV1 or H.264 MP4s; the same reviews with a third to a half of the CPU); `off` for ffmpeg's software decode (TOML: `gpu_frames = false`) |
| ffmpeg | `--ffmpeg` | the PATH's, else `ffmpeg/` in the data folder; `path` for the PATH's only |
| UI build | `--ui` | the repo's `ui/dist/server/browser` |
| Token | `--token` | none |
| Dev mode: no token ("Access") | `--dev` | off (TOML: `dev = true`) |

The same settings can go in a TOML file: `--config <file>`, or `aimview-server.toml` in the current folder. A flag
overrides the file. A relative path in the file starts at the file's folder. Write Windows paths in single quotes,
so the backslashes stay as they are:

```toml
host = "0.0.0.0"
port = 8770
data = 'D:\AimView\data'
vods = 'E:\OBS\KovOBS'
scenarios = ['C:\Program Files (x86)\Steam\steamapps\common\FPSAimTrainer\FPSAimTrainer\Saved\SaveGames\Scenarios']
device = "directml"
token = "a-long-random-string"
```

`aimview-server --help` lists every flag.

The data folder holds what the server writes: each recording's reviews and marks (`vod_app/`), uploads
(`vod_uploads/`), the detector labels of submitted cut-offs (`vod_model/hand/cutoff/`), the raw mouse logs (`mouse/`),
ffmpeg (`ffmpeg/`) and yt-dlp (`yt-dlp/`). The Python server used the same layout in `test_out/`, so the reviews and marks it kept are
read as they are. Python's scripts open the same library through `aimview-tool` (`python/aimview_tools.py`). An
upload is written into the uploads folder as it arrives, so it takes little memory however large it is.

## ffmpeg

The review reads the video's frames through ffmpeg. By default the server finds it as KovOBS does: first the PATH's
ffmpeg and ffprobe, when both run; else the ones in `ffmpeg/` in the data folder; else it downloads them there before
the first review: BtbN's build, which decodes AV1 with dav1d. gyan.dev's essentials build decodes AV1 with libaom, 2.5
times slower, and ignores `-skip_frame nokey`. `--ffmpeg <folder>` uses the ffmpeg in that folder (when it has none,
the PATH's, else a download into it). `--ffmpeg path` uses the PATH's only. The start-up log says which one it uses.

## Links

The UI's From a link (beside Upload) adds a recording from a link: a video's page on any site yt-dlp reads (YouTube,
Twitch VODs and clips, Medal, Streamable, X, Reddit, Vimeo, Google Drive, Dropbox...), or a video file's address.

- `POST /api/link/formats` with `{"url": ...}` answers `{title, duration, formats}`: the qualities, best first (the
  most pixels, then the highest frame rate), one for each frame size and frame rate, each with its `id`, `width`,
  `height`, `fps`, `codec` and `size` in bytes where yt-dlp knows it. Nothing is downloaded.
- `POST /api/link` with `{"url": ..., "format": <an id, or null for the best>}` answers at once with the new
  recording's `id`, its file name (`saved`) and its row. The file is named from the title: a KovOBS name
  (`<scenario> - <score> - <stamp>`) is kept, any other title becomes `<title> - <upload time>`.
- The download runs as the recording's job: `/api/job?id=` answers stage `downloading`, with `done` and `total` in
  megabytes (`yt-dlp` or `ffmpeg` while the server fetches them). The chosen quality's video and the best audio are
  merged into one MP4 with the review's ffmpeg, in a folder of its own (`.link-*` in the uploads folder), and moved
  into place when it is complete: the list never shows half a file. Then the job is gone (stage `none`). A failure ends
  it with stage `error` and yt-dlp's reason.

yt-dlp is found as ffmpeg is: the PATH's when it runs, else the official release from GitHub, downloaded once into
`yt-dlp/` beside ffmpeg's folder. Sites' terms may limit downloading their videos: add only videos you may download.

The UI in browser mode cannot read most sites' videos itself (the browser blocks it), so it asks this server: start it
with `bun run server`. The UI's Server for links setting holds its address (`http://127.0.0.1:8770` by default).

## The device

`--device` picks where the detector runs:

- `auto`: the GPU when there is one (DirectML on Windows, CUDA in a Linux build with the `cuda` feature), else the CPU.
- `directml`: any GPU on Windows.
- `cuda`: an NVIDIA GPU. Build with `cargo build -p aimview-server --release --features cuda`.
- `cpu`: the CPU.

The server logs the model and the device when it starts.

## Access

On a loopback address (`127.0.0.1`, `localhost`, `::1`) and with no token, only this machine gets in. The server
also refuses requests from web sites' pages: a request must name a loopback host, and come from a page on this
machine. A page on this machine on another port (the UI in browser mode, `http://localhost:4200`) may also read the
API's answers: they carry `Access-Control-Allow-Origin` for that page alone, never for another site's.

Any other address (`0.0.0.0`, a network address) needs a token. Without one, the server refuses to start. With a token,
every request must carry it:

- a browser: open `http://<server>:8770/?token=<token>` once. The server sets a cookie and the browser sends it from
  then on. Other sites' pages cannot use the cookie (it is `SameSite=Strict` and `HttpOnly`).
- a program: send the header `Authorization: Bearer <token>`.

Dev mode (`--dev`, or `dev = true` in the settings file) uses no token at all. With `host = "0.0.0.0"`, any device on
the local network gets in by this machine's address or name (`http://192.168.1.111:8770/`, `http://my-pc:8770/`,
`http://my-pc.local:8770/`). A web site's name that points at this machine is still refused (DNS rebinding), and so is
a request from another site's page. Any device on the network can then review, upload, download links and read the
recordings, so keep it to a network you trust. A token in the settings file is not used while dev mode is on.

A token holds letters, digits and `- . _ ~`. Make it long and random, for example
`python -c "import secrets; print(secrets.token_urlsafe(32))"`. Put it in the settings file rather than on the
command line, where other users of the machine can see it.

**The server speaks plain HTTP.** Nothing is encrypted: the token, the recordings and the reports cross the network as
they are. Use it on a network you trust (your home network, or a VPN), never open to the internet.

## Logs

One line per API request (method, path, status, time), never a body or a query. The UI's files, video ranges and
review progress (`/api/job`) are logged only when they fail. Each review adds one line when it ends: the recording, the
model and the device the detector ran on (with `--device auto`, the CPU when the GPU could not start it), and the time
or the error. `/api/job` also names the device (`device`) once the detector has loaded.
