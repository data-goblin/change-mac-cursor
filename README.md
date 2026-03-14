# cmc - Change Mac Cursor

Replace your macOS cursor with any PNG image. Based on [Mousecape](https://github.com/alexzielenski/Mousecape)'s private CoreGraphics APIs.

```bash
cargo install --path .

cmc --image cursor.png --size 32 --hotspot-x 2 --hotspot-y 2   # set
cmc --restore                                                    # reset
```

Example cursors from the [Warcraft II cursors](https://imgur.com/gallery/warcraft-ii-cursors-9ef8R) imgur gallery are included in `example-cursors/`.

<img src="example-cursors/orc-cursor.png" width="32" height="32" alt="Orc cursor" /> <img src="example-cursors/wc2-human-gauntlet.png" width="32" height="32" alt="Human cursor" />

Session-scoped (resets on logout). macOS only. Tested on Sequoia, Apple Silicon. MIT license.

---

Built with [Claude Code](https://claude.ai/claude-code).
