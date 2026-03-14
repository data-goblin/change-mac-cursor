# cmc - Change Mac Cursor

A small Rust CLI tool that replaces the macOS system cursor with a custom image.

Based on the cursor replacement mechanism from [Mousecape](https://github.com/alexzielenski/Mousecape) by Alex Zielenski. Uses private CoreGraphics Server (CGS) APIs to register custom cursor images with the WindowServer.

## Install

```bash
cargo install --path .
```

## Usage

```bash
# Replace the arrow cursor with a custom image
cmc --image cursor.png --size 32

# With custom hotspot (click point offset from top-left)
cmc --image cursor.png --size 48 --hotspot-x 2 --hotspot-y 2

# Replace a different cursor type
cmc --image hand.png --size 32 --cursor com.apple.coregraphics.OpenHand
```

### Options

| Flag | Default | Description |
|------|---------|-------------|
| `-i, --image` | required | Path to cursor image (PNG) |
| `-s, --size` | 32 | Cursor size in pixels (4-128) |
| `--hotspot-x` | 0 | Click point X offset |
| `--hotspot-y` | 0 | Click point Y offset |
| `-c, --cursor` | `com.apple.coregraphics.Arrow` | Cursor identifier to replace |
| `-r, --restore` | false | Restore default cursor (not yet implemented) |

### Common cursor identifiers

| Identifier | Cursor |
|-----------|--------|
| `com.apple.coregraphics.Arrow` | Default arrow |
| `com.apple.coregraphics.IBeam` | Text selection |
| `com.apple.coregraphics.OpenHand` | Grab hand |
| `com.apple.coregraphics.ClosedHand` | Grabbing hand |
| `com.apple.coregraphics.CrossHair` | Crosshair |
| `com.apple.coregraphics.PointingHand` | Link/pointer hand |

## Example

```bash
# Use the included orc cursor
cmc --image example-cursors/orc-cursor.png --size 32 --hotspot-x 2 --hotspot-y 2
```

## How it works

The tool calls `CGSRegisterCursorWithImages`, a private CoreGraphics function that registers cursor images directly with the macOS WindowServer. The replacement is:

- **Instant** - takes effect immediately, system-wide
- **Session-scoped** - resets on logout/restart
- **No root required** - runs as your regular user
- **No files modified** - purely in-memory replacement at the WindowServer level

## Limitations

- macOS only (uses private CoreGraphics APIs)
- Cursor resets on logout -- run again to re-apply
- `--restore` not yet implemented (log out to reset)
- Private APIs have no stability guarantee and may break on future macOS versions
- Tested on macOS Sequoia (Darwin 24.6.0), Apple Silicon

## License

MIT -- see [LICENSE](LICENSE).

## Acknowledgments

Cursor replacement mechanism based on [Mousecape](https://github.com/alexzielenski/Mousecape) by Alex Zielenski. The CGS API signatures were verified against Mousecape's `CGSCursor.h` and `apply.m`.
