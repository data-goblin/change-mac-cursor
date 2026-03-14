# Clippy Review: change-mac-cursor

## 1. Idiomatic Rust / Error Handling

**`load_cursor_image` returns `Option` but should return `Result`.**
The function swallows the actual error from `image::open(path).ok()?`. If a file is a corrupt PNG, a JPEG, or unreadable, the user gets "Failed to load or decode image" with zero diagnostic info. Switch to `Result<CGImage, Box<dyn std::error::Error>>` (or `anyhow::Result`) and propagate the real error.

**`fn load_cursor_image(path: &PathBuf)` -- Clippy lint `ptr_arg`.**
Should be `path: &Path` not `&PathBuf`. The `&PathBuf` auto-derefs but Clippy flags this as unnecessary indirection.

**`main()` uses `std::process::exit(1)` everywhere.**
Idiomatic Rust prefers returning `Result` from main:
```rust
fn main() -> Result<(), Box<dyn std::error::Error>> { ... }
```
This gives you `?` propagation and avoids scattered exit points that skip destructors.

**`register_cursor` returns `Result<(), i32>`.**
An i32 error code is opaque. At minimum, wrap it in a named error enum. Even a `struct CgsError(i32)` with a `Display` impl would improve ergonomics.


## 2. Dependencies

**The `image` crate is heavy for this use case.**
`image = "0.25"` pulls in decoders for JPEG, GIF, BMP, TIFF, WebP, AVIF, etc. You only need PNG decoding + resize. Two options:

- **Feature-gate it**: `image = { version = "0.25", default-features = false, features = ["png"] }` -- this alone should cut compile time and binary size significantly. You still get `image::open` and resize.
- **Go lighter**: Use `png` crate directly (~single decoder) + `fast_image_resize` or just nearest-neighbor resize in a few lines. Probably overkill unless binary size matters a lot.

Recommendation: at minimum add `default-features = false, features = ["png"]`. That's a one-line fix.

**`foreign-types = "0.5"` is only used for `ForeignType::as_ptr()`.**
This is one trait method. Check if `core-graphics` re-exports it (it depends on `foreign-types-shared`). If `CGImage` already has an `as_ptr()` through its own trait bounds, you can drop this dep.


## 3. Clippy Warnings That Would Fire

| Lint | Location | Fix |
|------|----------|-----|
| `clippy::ptr_arg` | `load_cursor_image(path: &PathBuf)` | Change to `&Path` |
| `clippy::cast_lossless` | `a as f32`, `r as f32` in premultiply loop | Use `f32::from(a)` |
| `clippy::manual_map` (possible) | The `match load_cursor_image` block in main | Not critical, but `unwrap_or_else` is cleaner |
| `clippy::needless_pass_by_value` | Not triggered here, but `cursor_name: &str` is correct | n/a |


## 4. Edge Cases

**Non-square images**: Handled -- `resize_exact` forces square output. But the user isn't warned. A 1920x1080 wallpaper silently becomes a 32x32 square with distorted aspect ratio. Consider: warn if aspect ratio differs significantly, or use `resize` (fit) + pad with transparency instead of `resize_exact`.

**Very large images**: `image::open` will happily try to decode a 100MP photo and allocate ~400MB for the RGBA buffer before resize. No size guard. For a cursor tool this is probably fine in practice but a 1-line check on file size or decoded dimensions would prevent surprise OOMs.

**Non-PNG files**: `image::open` will decode JPEG, BMP, etc. -- the docstring says "PNG with transparency" but there's no enforcement. A JPEG cursor would silently have no transparency (opaque alpha). Either:
- Check the file extension / magic bytes and reject non-PNG
- Or update the docs to say "any image format" and handle the alpha-less case explicitly

**Size = 0**: `--size 0` would create a 0x0 image. `resize_exact(0, 0, ...)` will either panic or produce garbage. Add a validation check.

**Hotspot out of bounds**: `--hotspot-x 999 --hotspot-y 999` on a 32x32 cursor is silently accepted. The WindowServer might handle it, but a bounds check with a warning would be friendlier.


## 5. Potential Panics

**`raw_data.chunks(4)` on line 121**: Safe only because `to_rgba8()` guarantees 4-byte-aligned data. Not a real risk, but an `chunks_exact(4)` would be semantically clearer and avoids the question.

**`CGImage::new` on line 143**: This calls into CoreGraphics. If `width` or `height` is 0 (from `--size 0`), behavior is undefined. Could panic, could segfault.

**`image::open` with a directory path**: Passing a directory as `--image` passes the `exists()` check but `image::open` will return an error, caught by `.ok()?`. Fine, but the error message is misleading ("Failed to load or decode image" vs "path is a directory").

**`CFArrayCreate` on line 186**: Raw unsafe call. If `image_ref` were null, this would be undefined behavior. In practice `CGImage` should never be null after construction, but there's no assertion.


## Summary of Recommended Changes (priority order)

1. `image = { ..., default-features = false, features = ["png"] }` -- free win
2. `load_cursor_image` -> return `Result` with real errors
3. Validate `--size` is > 0 and reasonable (e.g. 1..=256)
4. `path: &PathBuf` -> `path: &Path`
5. `chunks(4)` -> `chunks_exact(4)`
6. Warn or reject non-square source images
7. Consider dropping `foreign-types` if `as_ptr()` is available through `core-graphics`
