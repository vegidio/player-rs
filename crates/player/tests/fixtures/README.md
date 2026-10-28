# Test fixtures

## `testsrc-h264.mp4`

One second of FFmpeg's `testsrc` pattern, 160x120 at 25 fps, in H.264 (High profile, `yuv420p`). The hardware-decoding
tests in `tests/surface.rs` play it, because mpv's generated `av://lavfi` sources are raw video and never reach a
hardware decoder.

Generated with FFmpeg 9 and libx264:

```sh
ffmpeg -f lavfi -i testsrc=duration=1:size=160x120:rate=25 \
  -c:v libx264 -preset veryslow -profile:v high -pix_fmt yuv420p \
  -map_metadata -1 -fflags +bitexact -flags:v +bitexact -movflags +faststart -an \
  testsrc-h264.mp4
```

## `pkgconfig/`

`.pc` files for the build script's pkg-config resolution tests (`tests/pkgconfig.rs`).
