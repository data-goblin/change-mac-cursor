# Quartz -- CGS API Audit

Compared `src/main.rs` against Mousecape's `CGSCursor.h` and `apply.m`.

Reference declaration from Mousecape (`CGSCursor.h`):

```c
CGError CGSRegisterCursorWithImages(
    CGSConnectionID cid,        // typedef int CGSConnectionID
    char *cursorName,           // C string, NOT CFStringRef
    bool setGlobally,
    bool instantly,
    CGSize cursorSize,          // <-- position 5
    CGPoint hotspot,            // <-- position 6
    NSUInteger frameCount,      // <-- position 7
    CGFloat frameDuration,      // <-- position 8
    CFArrayRef imageArray,
    int *seed
);
```

Mousecape's actual call site (`apply.m`):

```objc
char *idenfifier = (char *)ident.UTF8String;
CGSRegisterCursorWithImages(
    CGSMainConnectionID(),
    idenfifier,          // char*, NOT CFStringRef
    true, true,
    size,                // CGSize -- position 5
    hotSpot,             // CGPoint -- position 6
    frameCount,          // NSUInteger -- position 7
    frameDuration,       // CGFloat -- position 8
    (__bridge CFArrayRef)images,
    &seed
);
```


## Findings

### BUG 1 -- CRITICAL: Parameter order is wrong

The Rust code has `frame_count` and `frame_duration` BEFORE `size` and `hotspot`. The correct order from the header and call site is:

```
Mousecape (correct):  cid, name, global, instant, SIZE, HOTSPOT, frameCount, frameDuration, images, seed
Rust code (wrong):    cid, name, global, instant, FRAMECOUNT, FRAMEDURATION, SIZE, HOTSPOT, images, seed
```

The Rust extern block declares parameters 5-8 as `(frame_count, frame_duration, size, hotspot)` but they must be `(size, hotspot, frame_count, frame_duration)`. This will cause the WindowServer to interpret CGSize bytes as frame count/duration and vice versa, producing undefined behavior or a crash.

**Fix:** Reorder the extern declaration to:

```rust
fn CGSRegisterCursorWithImages(
    connection: i32,               // fixed type too, see bug 2
    cursor_name: *const i8,        // fixed type too, see bug 3
    set_globally: bool,
    instantly: bool,
    size: CGSize,                  // <-- moved before frame_count
    hotspot: CGPoint,              // <-- moved before frame_duration
    frame_count: usize,           // fixed type too, see bug 4
    frame_duration: f64,
    images: core_foundation::array::CFArrayRef,
    seed: *mut i32,
) -> i32;
```

And update the call site to match.


### BUG 2 -- WRONG TYPE: CGSConnectionID is `int`, not `u32`

`CGSConnection.h` defines:

```c
typedef int CGSConnectionID;
```

That is a **signed** 32-bit integer. The Rust code declares `CGSMainConnectionID() -> u32` and `connection: u32`. This should be `i32` in both places.

On arm64 macOS this is unlikely to cause a visible bug (the connection ID is typically a small positive number), but it is technically an ABI mismatch. If the WindowServer ever returns a negative error sentinel from `CGSMainConnectionID`, the `connection == 0` check would miss it.

**Fix:** Change `u32` to `i32` for `CGSMainConnectionID` return type and `connection` parameter.


### BUG 3 -- WRONG TYPE: cursor_name is `char *`, not `CFStringRef`

The header declares `char *cursorName`. Mousecape converts NSString to `char *` via `.UTF8String`.

The Rust code passes `CFStringRef`, which is a pointer to an opaque CFString object, NOT a C string. The WindowServer will try to read it as a null-terminated UTF-8 byte sequence and will read garbage (the CFString object's internal struct fields).

This is almost certainly why the cursor registration fails or produces unexpected behavior.

**Fix:** Use `std::ffi::CString` to create a null-terminated C string:

```rust
let c_name = std::ffi::CString::new(cursor_name).unwrap();
// pass c_name.as_ptr() as *const i8
```

And change the extern parameter type from `CFStringRef` to `*const std::ffi::c_char`.


### BUG 4 -- WRONG TYPE: frame_count is `NSUInteger`, not `i32`

`NSUInteger` on 64-bit macOS is `unsigned long`, which is 8 bytes (`u64`/`usize`). The Rust code uses `i32` (4 bytes). This is a size mismatch in the calling convention -- the function expects an 8-byte value in the register/stack slot, but only 4 bytes are provided, which corrupts subsequent arguments.

On arm64 ABI this is particularly dangerous because each argument occupies its own register, so a 4-byte value zero-extended into a 64-bit register might accidentally work, but it is still incorrect and fragile.

**Fix:** Change `frame_count: i32` to `frame_count: usize` (which is `u64` on 64-bit).


### BUG 5 -- MINOR: CGFloat is `double` on 64-bit, matches f64

`frame_duration: f64` is correct. `CGFloat` is `double` (8 bytes) on arm64 macOS. No issue here.


### OK: CGImage to CFArray

The CFArray creation is correct:

```rust
let image_ref = image.as_ptr() as *const std::ffi::c_void;
let images = CFArrayCreate(null(), &image_ref, 1, &kCFTypeArrayCallBacks);
```

- `as_ptr()` on `CGImage` returns the underlying `CGImageRef` (a `*mut CGImage` C pointer)
- Casting to `*const c_void` is correct for CFArray's `const void **values` parameter
- `&image_ref` gives a pointer-to-pointer, which is correct (`const void **`)
- Count of 1 is correct for a single-frame cursor
- `kCFTypeArrayCallBacks` is correct for CFType objects (retains/releases properly)

One minor concern: the `images` CFArrayRef is never released (`CFRelease`). This is a small memory leak but not a correctness bug.


### OK: CGSize and CGPoint repr(C)

Both structs use `#[repr(C)]` with `f64` fields, matching the C `CGSize { CGFloat width, height }` and `CGPoint { CGFloat x, y }` on 64-bit. Correct.


### OK: CGImage creation

The premultiplied alpha handling and `CGImageAlphaPremultipliedLast` bitmap info are correct for RGBA byte order with premultiplied alpha. This matches what CoreGraphics cursor registration expects.


## Summary of Required Fixes (priority order)

| # | Severity | Issue | Current | Correct |
|---|----------|-------|---------|---------|
| 1 | CRITICAL | Parameter order wrong | frameCount, frameDuration, size, hotspot | size, hotspot, frameCount, frameDuration |
| 2 | CRITICAL | cursor_name type wrong | CFStringRef | *const c_char (C string) |
| 3 | MODERATE | CGSConnectionID type wrong | u32 | i32 |
| 4 | MODERATE | frame_count type wrong | i32 (4 bytes) | usize (8 bytes) |
| 5 | MINOR | CFArray not released | -- | Call CFRelease after use |
