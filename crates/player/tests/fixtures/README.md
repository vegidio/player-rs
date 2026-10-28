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

## `testsrc-h264-aac.mp4`

Five seconds of the same pattern with a 440 Hz tone (AAC, mono, 8 kHz). `renderer_created_after_load` in
`tests/surface.rs` plays it, because it must be seekable: mpv re-selects the video track with a seek to the current
position, which the generated `av://lavfi` sources cannot do.

Generated with FFmpeg 9, libx264 and FFmpeg's AAC encoder:

```sh
ffmpeg -f lavfi -i testsrc=duration=5:size=160x120:rate=25 -f lavfi -i sine=frequency=440:duration=5:sample_rate=8000 \
  -c:v libx264 -preset veryslow -profile:v high -pix_fmt yuv420p -c:a aac -b:a 16k -ac 1 \
  -map_metadata -1 -fflags +bitexact -flags:v +bitexact -flags:a +bitexact -movflags +faststart -shortest \
  testsrc-h264-aac.mp4
```

## `pkgconfig/`

`.pc` files for the build script's pkg-config resolution tests (`tests/pkgconfig.rs`).
