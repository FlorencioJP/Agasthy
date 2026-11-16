#!/usr/bin/env bash
# README GIF from the rendered MP4: 800px, 8 fps, 128 colours (about 10 MB for ~60 s).
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p ../../docs/media
ffmpeg -y -loglevel error -i out/agasthy-demo.mp4 -vf "fps=8,scale=800:-1:flags=lanczos,palettegen=max_colors=128:stats_mode=diff" out/palette.png
ffmpeg -y -loglevel error -i out/agasthy-demo.mp4 -i out/palette.png -lavfi "fps=8,scale=800:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=none:diff_mode=rectangle" ../../docs/media/agasthy-demo.gif
ls -la ../../docs/media/agasthy-demo.gif
