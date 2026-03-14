# Crash's QA Review - change-mac-cursor

## 1. No Alpha Channel

**Severity: Low (handled implicitly)**

`load_cursor_image` calls `.to_rgba8()` on line 116, which forces conversion to RGBA regardless of input format. A JPEG or opaque PNG with no alpha channel gets alpha=255 for every pixel. The premultiply loop then multiplies RGB by 1.0, so values pass through unchanged. This is fine -- the cursor just won't have any transparency. No crash, no corruption.

**However**: the CLI help says "PNG with transparency" but there's zero validation that the image actually has alpha. A user passing a JPEG gets a cursor that works but has a solid rectangular background where they probably expected transparency. This is a UX paper cut, not a bug.


## 2. CGSMainConnectionID Returns 0 (SSH / Headless)

**Severity: Medium (handled, but incomplete)**

Line 176 checks `connection == 0` and prints a helpful error + returns `Err(-1)`. Good. But the caller in `main()` (line 265) prints "Failed to register cursor (error code: -1)" and suggests "accessibility permissions or SIP adjustments" -- that message is misleading for the headless case. The real CGS error codes will also be negative integers, so -1 from the manual check collides with possible real error codes. If CGS ever returned -1 naturally, you couldn't distinguish the two failure modes.

**Risk**: misleading error message in SSH/CI/headless scenarios. User gets told to check SIP when the real problem is no GUI session.


## 3. CGSRegisterCursorWithImages Failure - Crash or Graceful?

**Severity: Medium (mostly handled, with caveats)**

The return code is checked on line 216. Non-zero returns `Err(result)`, which `main()` handles with an error message and `exit(1)`. So it degrades -- no panic, no crash.

**But**: there's no documentation of what error codes mean. The private API could also theoretically write garbage to the `seed` output pointer even on failure. Since `seed` is a stack variable that's never read after the call, this is harmless, but it's sloppy.

**Bigger concern**: what if the API doesn't just return an error code but actually crashes the process? Private APIs have no stability guarantees. On a future macOS version, passing unexpected parameters (e.g., a mismatched `size` vs actual image dimensions) could segfault inside the framework. There's no guard against this -- you'd need a signal handler or at minimum document the risk.


## 4. CFArray Leak

**Severity: High (confirmed memory leak)**

Line 186-191 creates a `CFArrayRef` via `CFArrayCreate`. This is a Create-rule CF object -- the caller owns it and must call `CFRelease` when done. **It is never released.** The function returns without calling `CFRelease(images)`.

For a CLI tool that runs once and exits, the OS reclaims everything, so this is practically harmless. But it's still a bug:
- If this code were ever used as a library or called repeatedly (e.g., animated cursor with frame updates), it would leak on every call.
- It's bad practice and would fail any CF memory audit.

**Fix**: add `core_foundation::base::CFRelease(images as *const _)` after the `CGSRegisterCursorWithImages` call, or wrap it in a safe `CFArray` type that drops automatically.

Additionally: `cf_name` (the `CFString`) is created with `CFString::new()` which returns a Rust wrapper that implements `Drop` via `TCFType`, so that one is fine. Only the raw `CFArrayRef` leaks.


## 5. Apple Silicon vs Intel

**Severity: Low-Medium (no immediate issue, latent risk)**

The CGS private API symbols are present in CoreGraphics.framework on both architectures. The `#[link]` directive links by framework name, which works for both arm64 and x86_64. The `#[repr(C)]` structs (`CGSize`, `CGPoint`) use `f64` which matches `CGFloat` on 64-bit (both AS and Intel macOS are 64-bit).

**Potential concern**: `CGSMainConnectionID` is declared as returning `u32`. The actual return type in Apple's private headers is `CGSConnectionID` which is typically `int` (i32). Using `u32` means a hypothetical error sentinel of -1 would be interpreted as `u32::MAX` (4294967295), which would pass the `== 0` check and proceed with a garbage connection ID. This could cause undefined behavior inside `CGSRegisterCursorWithImages`.

**Recommendation**: change the return type to `i32` and check for `<= 0`.

No Rosetta-specific issues anticipated -- the framework calls go through the normal dyld path.


## 6. Extreme Image Sizes

### 1px image

**Severity: Low**

`resize_exact(target_size, target_size, Lanczos3)` will upscale a 1x1 image to 32x32 (or whatever `--size` is). Lanczos3 on a 1px source will produce a blurred single-color square. It works, but the result is useless. No crash.

### 10000px image

**Severity: Medium (resource exhaustion)**

A 10000x10000 RGBA image is ~400MB in memory. `resize_exact` would scale it down to 32x32, but the full image is loaded into memory first by `image::open()`. This could cause OOM on constrained systems.

More critically: if the user passes `--size 10000`, the tool will:
1. Load the image (fine)
2. Resize to 10000x10000 = 100M pixels = 400MB RGBA buffer
3. Premultiply alpha on 400MB (another 400MB allocated at line 120)
4. Create a CGImage backed by 400MB
5. Pass it to the WindowServer

The WindowServer almost certainly has undocumented limits on cursor dimensions. macOS cursors are typically 16-128px. Passing a 10000px cursor image to a private API with no size validation is playing with fire -- it could hang the WindowServer, cause massive VRAM allocation, or just fail silently.

**No upper bound is enforced on `--size`.** There's also no lower bound -- `--size 0` would attempt a 0x0 resize which would either panic in the image crate or produce a degenerate CGImage.

**Recommendation**: clamp `--size` to something like 4-256 with a hard error outside that range.


## 7. Bonus Findings

### Hotspot out of bounds
No validation that `hotspot_x` and `hotspot_y` are within `[0, size)`. A hotspot of (999, 999) on a 32px cursor is undefined behavior in the private API. Could cause the click point to be completely offset from the visible cursor, effectively making the system unusable until logout.

### --restore is a dead flag
Line 231-234: `--restore` is advertised but just prints an error and exits. The flag exists in the CLI parser, so users will try it and be confused. Should either be implemented or removed from the parser.

### No error context from image::open
Line 108: `.ok()?` discards the actual error from image loading. If a file exists but isn't a valid image (e.g., a text file with .png extension), the user gets "Failed to load or decode image" with no details about why. The original error (e.g., "invalid PNG header") is swallowed.

### Race condition on file existence check
Lines 236-239 check `args.image.exists()` then later `image::open()` reads it. TOCTOU race -- the file could be deleted between the check and the open. Minor, since `image::open` would fail gracefully anyway, but the existence check is redundant and gives false confidence.
