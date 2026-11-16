#!/usr/bin/env bash
# README GIF (800px, 8 fps, 128 colours, about 10 MB) and a GitHub-uploadable
# MP4 (1080p, under 10 MB) from the rendered video.
set -euo pipefail
cd "$(dirname "$0")"
mkdir -p ../../docs/media
ffmpeg -y -loglevel error -i out/agasthy-demo.mp4 -vf "fps=8,scale=800:-1:flags=lanczos,palettegen=max_colors=128:stats_mode=diff" out/palette.png
ffmpeg -y -loglevel error -i out/agasthy-demo.mp4 -i out/palette.png -lavfi "fps=8,scale=800:-1:flags=lanczos[x];[x][1:v]paletteuse=dither=none:diff_mode=rectangle" ../../docs/media/agasthy-demo.gif
# Under GitHub's 10 MB upload limit for videos in a README (drag into the editor).
ffmpeg -y -loglevel error -i out/agasthy-demo.mp4 -c:v libx264 -preset slow -crf 27 -pix_fmt yuv420p -movflags +faststart -an out/agasthy-demo-github.mp4
ls -la ../../docs/media/agasthy-demo.gif out/agasthy-demo-github.mp4
