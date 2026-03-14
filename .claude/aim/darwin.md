# Security Audit: change-mac-cursor

**Auditor:** Darwin (macOS security)
**Target:** `src/main.rs` -- CGS-based cursor replacement tool
**Host:** macOS 15.6.1 (Sequoia), SIP enabled, Darwin 24.6.0


## 1. Private CGS API Usage on Modern macOS

The tool uses two undocumented WindowServer APIs:
- `CGSMainConnectionID()` -- gets the calling process's connection to the WindowServer
- `CGSRegisterCursorWithImages()` -- registers cursor images globally

### Status on Sequoia (macOS 15)

These APIs still exist in CoreGraphics.framework and are still used by tools like Mousecape. Apple has not removed them, but they carry no stability guarantee. Any point release could:
- Change the function signature (causing undefined behavior, not a compile error -- these are runtime-linked)
- Add entitlement requirements
- Remove them entirely

**Realistic risk:** Low-to-medium. Apple has left these alone for years, but Sequoia has been tightening private API access. The tool should gracefully handle a non-zero return from `CGSRegisterCursorWithImages` (it does, line 216-219) and a zero connection ID (it does, line 176-179). That's the right approach.

**Missing:** There is no check for the macOS version at runtime. If Apple changes the ABI, calling the function with the wrong signature could corrupt the stack or crash rather than returning an error code. Consider adding `sysctl` or `NSProcessInfo` version gating as a safety net.


## 2. SIP, TCC, and Entitlements

### SIP (System Integrity Protection)

SIP does **not** block this tool. CGS APIs communicate with the WindowServer via Mach IPC -- they do not modify protected system files or inject into protected processes. The cursor change is session-scoped, held in WindowServer memory, not written to disk. SIP is irrelevant here.

### TCC (Transparency, Consent, and Control)

On Sequoia, Apple expanded TCC to cover screen recording and input monitoring. `CGSRegisterCursorWithImages` operates at the WindowServer level but it is **registering** an image, not capturing screen content or monitoring input. As of macOS 15.6, no TCC prompt is triggered.

However: if Apple ever reclassifies cursor manipulation as an accessibility or input-monitoring action, TCC would block it silently (returning an error code from CGS). The tool handles this case already.

### Entitlements / Notarization

- The binary is not sandboxed (no entitlements file in the repo), which is correct -- sandboxed apps cannot talk to CGS.
- If distributed, Apple notarization could flag private API usage. The `CGSRegisterCursorWithImages` symbol is detectable by static analysis tools Apple uses during notarization. This would likely get rejected.
- For personal/developer use, this is fine. For distribution, it is a problem.


## 3. Malicious Exploitation Potential

### As an attack vector

A malicious actor who can execute code as the current user can already do far worse things than change a cursor. That said, this tool's technique could be used for:

- **Phishing/UI spoofing:** Replace the arrow cursor with a fake "text input" cursor to trick users into clicking in unexpected places. Realistic but low-value compared to other attack surfaces.
- **Cursor misdirection:** Set the hotspot to a different location than the visual cursor tip (e.g., visual cursor points at "Cancel" but hotspot is over "Delete All"). This is the most plausible abuse case.
- **Denial of usability:** Replace cursor with a 1x1 transparent image, effectively hiding it. User can still use the mouse but cannot see it.

### Mitigations already in place

- Requires local code execution as the logged-in GUI user
- Resets on logout/restart (session-scoped)
- No persistence mechanism -- cannot survive a reboot

### What's NOT a concern

- Cannot escalate privileges
- Cannot affect other user sessions
- Cannot modify system cursor files on disk (SIP protects those)


## 4. Can This Brick the Cursor?

**Short answer:** No, not permanently. But it can make the cursor unusable until logout.

### Scenarios

| Scenario | Effect | Recovery |
|----------|--------|----------|
| Replace arrow with transparent 1px image | Cursor invisible | Logout, or `killall WindowServer` (logs you out) |
| Replace arrow with 1000x1000 image | Oversized cursor, likely clipped by WindowServer | Logout |
| Set hotspot outside image bounds | Click position misaligned from visual | Logout |
| Replace all cursor types (arrow, ibeam, crosshair, etc.) | All cursors affected | Logout |
| Crash during CGS call | No effect -- WindowServer is a separate process | None needed |

The `--restore` flag is currently unimplemented (line 231-234). This is the biggest practical concern. If a user replaces their cursor and wants it back, their only option is logout or `killall WindowServer`.

**Recommendation:** Implement restore before distributing this. The restore path would call `CGSRegisterCursorWithImages` with the default system cursor image, or use `CGSSetSystemDefinedCursor` if available.

### Memory leak concern

Line 185-192: The `CFArrayCreate` call creates a CFArray that is never released. This leaks ~48 bytes per invocation. Irrelevant for a CLI tool that exits immediately, but worth noting if this code is ever pulled into a long-running process.


## 5. Regular User vs Root

### Running as regular user (recommended)

- `CGSMainConnectionID()` returns the connection for the **current GUI session owner**. It works as the logged-in user with no elevation needed.
- The cursor change only affects the calling user's session.
- This is the correct way to run the tool.

### Running as root

- If run as root via `sudo` from a terminal within a GUI session, it will likely still work because the terminal inherits the GUI session's SecuritySessionID.
- If run as root from a non-GUI context (SSH, launchd), `CGSMainConnectionID()` returns 0 and the tool exits gracefully (line 176-179).
- Running as root provides no benefit and no additional risk -- the CGS connection is session-scoped regardless.

**Verdict:** No reason to ever run this as root. The tool correctly operates at user-session scope.


## Summary

| Area | Risk Level | Notes |
|------|------------|-------|
| Private API stability | Medium | Could break silently on any macOS update |
| SIP/TCC blocking | Low | Not currently blocked; could change |
| Malicious exploitation | Low | Requires existing code execution; session-scoped |
| Cursor bricking | Low | Recoverable via logout; no permanent damage possible |
| User vs root | None | Works correctly as regular user; root adds nothing |

### Priority Recommendations

1. **Implement `--restore`** -- this is the single most impactful improvement for usability and safety
2. **Add macOS version check** -- warn or bail on untested versions to avoid ABI mismatch crashes
3. **Release the CFArray** (line 185-192) -- minor, but good hygiene
4. **Do not attempt notarization** -- private API usage will be flagged; distribute as a developer tool only
