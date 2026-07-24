# Sim Racing Display Fixer

A tiny, free Windows app that keeps your sim rig's screens at the resolution and refresh rate you
want. Set it once, and it quietly puts your display back whenever Windows or a GPU driver knocks it
out of place.

Built with Tauri (Rust + WebView2). The whole heal engine is a ~200 KB native binary, so the app
stays lightweight and has no .NET or other runtime dependency.

---

## The problem

Triple-screen sim rigs (NVIDIA Surround, or any multi-monitor setup) have a nasty habit of losing
their resolution or refresh rate. The classic case: a 3x 4K Surround span that should be
**11520x2160 @ 120 Hz** comes back after a shutdown or cold boot as **5760x1080 @ 60 Hz**, and once
it collapses it stays stuck at that low mode until you reinstall the graphics driver. The same class
of problem hits normal setups too: a monitor that drops to a lower resolution after a reboot, or a
refresh rate that quietly resets from 120 Hz back to 60 Hz.

## Why it happens (root cause)

On a cold boot the DisplayPort links negotiate down to the universal 1920x1080 @ 60 Hz "safe" mode
**before the GPU driver loads**. The driver then loads and re-applies your saved Surround grid on
top of those degraded per-tile modes. The result is a desync: the NVIDIA layer still reports a full
1x3 grid of 4K panels, while Windows only offers modes up to 5760 wide, so the wide mode is
genuinely no longer available and cannot be re-selected normally.

This was root-caused and verified on an **RTX 5070 Ti with driver 610.74**, where every "normal"
recovery is broken:

- Re-applying the mode through the Windows CCD API returns `ERROR 87` (the wide mode is not offered).
- Rebuilding the NVIDIA Surround grid through NVAPI fails with a driver struct-version regression.
- Toggling the monitor device does nothing (Surround hides the physical panels behind one virtual
  device).

The only thing that reliably brings it back on that driver, short of a full reinstall, is a **full
restart of the display adapter**.

## How it fixes it

The fixer is **profile-based**, so it works on any GPU and any layout instead of hardcoding one
resolution:

1. **Set it once.** It captures your current display config (every monitor, resolution, refresh,
   and position) through the Windows CCD API and saves it as your target.
2. **It enforces that target.** Whenever your display drifts from the saved profile, it fixes it in
   two escalating steps:
   - **Re-apply the saved config (CCD).** Handles the common cases on any GPU: a monitor that
     dropped resolution, a refresh rate reset, a rearranged layout.
   - **Restart the display adapter.** If the mode re-apply cannot fix it (the Surround-collapse
     case above), it disables and re-enables the display adapter. That forces a full driver
     re-init and DisplayPort link re-train with the panels already awake, then re-applies your saved
     config. This is the step that recovers the collapsed 3x 4K span, and it is vendor-neutral.

**Silent fixing at startup** runs as an elevated logon task (the adapter restart needs admin, so the
task carries the elevation, registered once when you turn on "Auto-fix at startup" - no admin prompt
on every boot). Open the app any time to check status, re-capture your target, fix now, or update.

## What it works on

- **Windows 10 and 11**, 64-bit.
- **Any GPU: NVIDIA, AMD, or Intel.** The detect-and-heal path (CCD + adapter restart) is
  vendor-neutral. The NVIDIA Surround collapse is specifically handled.
- **Single or multiple monitors**, Surround or standalone. The whole desktop topology is captured as
  the target, so any resolution / refresh / layout is supported.
- **Requires the WebView2 runtime**, which ships with Windows 11 and installs automatically on
  Windows 10.

Validated on: an **NVIDIA RTX 5070 Ti (driver 610.74)** triple-4K Surround rig at 11520x2160 @ 120 Hz,
and on an **AMD Radeon 880M** laptop - two very different setups, same profile-based fix.

## Install

Download the latest setup `.exe` from the [Releases](../../releases) page and run it. After that the
app updates itself: open it any time and it offers a one-click update when a new version is out.

> First-download note: until the installer is code-signed, Windows SmartScreen may warn about an
> "unknown publisher." Click "More info" then "Run anyway." Auto-update is cryptographically signed
> and unaffected.

## Build from source

Prereqs: Rust (MSVC toolchain) and the Tauri CLI (`cargo install tauri-cli --version "^2.0.0"`). No
Node needed - the UI is static.

```bash
cargo tauri build      # NSIS installer under target/release/bundle/nsis/
cargo tauri dev        # run locally
```

The core engine also has a headless test CLI:

```bash
cargo run -p lunis-display-core --bin displaycore -- status   # or: capture | fix | restart
```

## Auto-update and releasing

Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds, signs the update artifacts,
publishes a GitHub Release, and emits `latest.json` (the manifest the app polls). Two repo secrets
are required (see the workflow header): `TAURI_SIGNING_PRIVATE_KEY` and
`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.

Update integrity is signed for free with a minisign key. That is separate from Windows code-signing
for SmartScreen; until the installer is code-signed (for example with Azure Trusted Signing), the
first download shows an "unknown publisher" warning while auto-update still works.
