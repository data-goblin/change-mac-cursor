# Ferris -- Security Audit of main.rs

Audited: `src/main.rs` (274 lines)
Reference: Mousecape `CGSInternal/CGSCursor.h` line 52, `apply.m` lines 23-32


## CRITICAL: Parameter Order Mismatch in CGSRegisterCursorWithImages (Lines 35-46)

This is an **undefined behavior** bug that will cause crashes or silent corruption.

### Mousecape's verified signature (CGSCursor.h:52):

```c
CGSRegisterCursorWithImages(
    CGSConnectionID cid,
    char *cursorName,
    bool setGlobally,
    bool instantly,
    CGSize cursorSize,     // <-- size FIRST
    CGPoint hotspot,       // <-- hotspot SECOND
    NSUInteger frameCount, // <-- frameCount THIRD
    CGFloat frameDuration, // <-- frameDuration FOURTH
    CFArrayRef imageArray,
    int *seed
);
```

### Your Rust declaration (lines 35-46):

```rust
fn CGSRegisterCursorWithImages(
    connection: u32,
    cursor_name: CFStringRef,
    set_globally: bool,
    instantly: bool,
    frame_count: i32,      // <-- WRONG: should be CGSize
    frame_duration: f64,   // <-- WRONG: should be CGPoint
    size: CGSize,          // <-- WRONG: should be NSUInteger (frameCount)
    hotspot: CGPoint,      // <-- WRONG: should be CGFloat (frameDuration)
    images: CFArrayRef,
    seed: *mut i32,
);
```

**The order of parameters 5-8 is wrong.** Mousecape puts `size, hotspot, frameCount, frameDuration`. Your code puts `frameCount, frameDuration, size, hotspot`. This passes garbage values to every parameter -- structs where scalars are expected and vice versa. The ABI will read wrong memory regions.

**Fix:** Reorder to match Mousecape:

```rust
fn CGSRegisterCursorWithImages(
    connection: u32,
    cursor_name: *const c_char,  // see finding #2
    set_globally: bool,
    instantly: bool,
    size: CGSize,
    hotspot: CGPoint,
    frame_count: usize,          // NSUInteger = usize
    frame_duration: f64,
    images: CFArrayRef,
    seed: *mut i32,
) -> i32;
```

Also update the call site at lines 201-213 accordingly.


## CRITICAL: Wrong Type for cursor_name -- CFStringRef vs char* (Line 37)

Mousecape declares `cursorName` as `char *` (a C string pointer), not `CFStringRef`. These are fundamentally different types:
- `CFStringRef` is a pointer to a CoreFoundation object with refcount, vtable, etc.
- `char *` is a raw null-terminated byte buffer.

The WindowServer expects a C string. Passing a `CFStringRef` means it will interpret the CFString object header bytes as a string, leading to undefined behavior.

**Lines affected:** 37 (declaration), 204 (call site using `cf_name.as_concrete_TypeRef()`)

**Fix:** Use `CString` instead:

```rust
// Declaration (line 37):
cursor_name: *const std::ffi::c_char,

// Call site (lines 181, 204):
let c_name = std::ffi::CString::new(cursor_name).expect("cursor name contains null byte");
// ...
c_name.as_ptr(),
```


## HIGH: CGSConnectionID Type Mismatch (Line 21)

Mousecape declares `CGSConnectionID` as `int` (i.e., `i32` in Rust), defined in `CGSConnection.h:28`:

```c
typedef int CGSConnectionID;
```

Your code uses `u32` on lines 21 and 36. While this works on most ABIs for same-width integers, it is technically incorrect and the zero-check on line 176 (`connection == 0`) would not catch a negative error value if the API ever returned one (Mousecape defines `kCGSNullConnectionID = 0` but errors could be negative).

**Fix:** Change `u32` to `i32` on lines 21 and 36.


## HIGH: frame_count Type Mismatch (Line 40)

Mousecape declares `frameCount` as `NSUInteger`, which is `usize` in Rust (8 bytes on 64-bit). Your code uses `i32` (4 bytes). This ABI width mismatch will cause the stack frame to be misread by the callee.

**Fix:** Use `usize` for `frame_count`.


## MEDIUM: CFArray Resource Leak (Lines 185-192)

The `CFArrayRef` created by `CFArrayCreate` at line 186 is never released. `CFArrayCreate` follows the Create Rule -- the caller owns the returned reference and must call `CFRelease` when done.

The function returns without releasing `images`, leaking the CFArray on every call.

**Fix:** Add cleanup after the CGS call:

```rust
let result = unsafe { CGSRegisterCursorWithImages(/* ... */) };

// Release the CFArray we created
unsafe { core_foundation::base::CFRelease(images as *const _) };
```

Or better, wrap it in a safe `CFArray` type that drops automatically.


## MEDIUM: No Validation of CGImage Pixel Format Assumptions (Lines 119-131)

The premultiply loop at lines 121-131 assumes `raw_data.len()` is divisible by 4. While `to_rgba8()` guarantees RGBA output, a corrupted or zero-dimension image could produce an empty or misaligned buffer. The `chunks(4)` call would silently drop a remainder chunk of 1-3 bytes.

**Fix:** Add an assertion:

```rust
assert!(raw_data.len() % 4 == 0, "RGBA buffer length not aligned to 4 bytes");
```

Or use `chunks_exact(4)` which panics on remainder, making the contract explicit.


## LOW: Premultiply Precision Loss (Lines 127-129)

The float-based premultiply `(r as f32 * af) as u8` truncates rather than rounds. For example, `r=255, a=128` gives `af=0.50196`, result `128.0` which is fine, but `r=255, a=1` gives `af=0.00392`, result `0.999..` truncated to `0` instead of rounded to `1`.

This is cosmetically minor but worth noting. The integer-exact formula avoids float entirely:

```rust
premultiplied.push(((r as u16 * a as u16 + 127) / 255) as u8);
```


## LOW: Hotspot Coordinates Not Validated (Lines 255-258)

`hotspot_x` and `hotspot_y` accept any `f64`, including negative values or values larger than the cursor size. Passing out-of-bounds hotspot coordinates to CGS could cause unexpected click targeting.

**Fix:** Clamp or validate against `size`:

```rust
if args.hotspot_x < 0.0 || args.hotspot_x >= size as f64
    || args.hotspot_y < 0.0 || args.hotspot_y >= size as f64 {
    eprintln!("Hotspot must be within cursor bounds [0, {})", size);
    std::process::exit(1);
}
```


## INFO: bool ABI in extern "C" (Lines 38-39)

Rust `bool` in `extern "C"` is defined as `i8` (0 or 1). The Mousecape header uses C99 `bool` (also `_Bool`, 1 byte). This is compatible on macOS ABI, so this is not a bug, but worth documenting that it relies on platform-specific ABI agreement. Using `std::ffi::c_bool` (stabilized in Rust 1.82) would be more explicit.


## Summary

| # | Severity | Issue | Lines |
|---|----------|-------|-------|
| 1 | CRITICAL | Parameter order wrong (size/hotspot vs frameCount/frameDuration swapped) | 35-46, 201-213 |
| 2 | CRITICAL | cursor_name should be `char*` not `CFStringRef` | 37, 181, 204 |
| 3 | HIGH | CGSConnectionID should be `i32` not `u32` | 21, 36 |
| 4 | HIGH | frame_count should be `usize` not `i32` (NSUInteger width mismatch) | 40 |
| 5 | MEDIUM | CFArray from CFArrayCreate never released | 185-192 |
| 6 | MEDIUM | No assertion that pixel buffer is 4-byte aligned | 121 |
| 7 | LOW | Float truncation in premultiply | 127-129 |
| 8 | LOW | Hotspot coordinates not validated against bounds | 255-258 |
| 9 | INFO | bool ABI compatibility note | 38-39 |

Findings 1 and 2 are almost certainly causing this tool to either crash or silently fail at runtime. If you have observed it "not working," those two are the reason.
