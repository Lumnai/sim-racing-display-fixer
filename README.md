# Sim Display Fixer

A tiny, free Windows app that keeps your sim rig's screens at the resolution and refresh rate you
want. Set it once, and it quietly puts your display back whenever Windows or a GPU driver knocks it
out of place.

Built with **Rust and [Slint](https://slint.dev)** as a fully native app. There is no browser engine,
no .NET, and no other runtime to install: one self-contained `.exe` that sits at roughly **4 MB of
memory** while its window is open, and nothing at all when it is closed.

---

## The problem

Triple-screen sim rigs have a nasty habit of losing their resolution or refresh rate. The classic
case: a 3x 4K NVIDIA Surround span that should be **11520x2160 @ 120 Hz** comes back after a shutdown
or cold boot as **5760x1080 @ 60 Hz**, and once it collapses it stays stuck at that low mode until you
reinstall the graphics driver. The same class of problem hits ordinary setups too: a monitor that
drops to a lower resolution after a reboot, or a refresh rate that quietly resets from 120 Hz to 60.

## Why it happens (root cause)

On a cold boot the DisplayPort links negotiate down to the universal 1920x1080 @ 60 Hz "safe" mode
**before the GPU driver loads**. The driver then loads and re-applies your saved Surround grid on top
of those degraded per-tile modes. The result is a desync: the NVIDIA layer still reports a full 1x3
grid of 4K panels, while Windows only offers modes up to 5760 wide, so the wide mode is genuinely no
longer available and cannot be re-selected normally.

This was root-caused and verified on an **RTX 5070 Ti with driver 610.74**, where every "normal"
recovery is broken:

- Re-applying the mode through the Windows CCD API returns `ERROR 87` (the wide mode is not offered).
- Rebuilding the NVIDIA Surround grid through NVAPI fails with a driver struct-version regression.
- Toggling the monitor device does nothing (Surround hides the physical panels behind one virtual
  device).

The only thing that reliably brings it back on that driver, short of a full driver reinstall, is a
**full restart of the display adapter**.

## How it fixes it

The fixer is **profile-based**, so it works on any GPU and any layout instead of hardcoding one
resolution:

1. **It learns your target.** On first run it adopts whatever your display is set to. After that, any
   mode you apply and keep becomes the new target.
2. **It enforces that target.** Whenever your display drifts from it, the fix escalates in two steps:
   - **Re-apply the saved config (Windows CCD).** Handles the common cases on any GPU: a monitor that
     dropped resolution, a refresh rate reset, a rearranged layout.
   - **Restart the display adapter.** If the mode re-apply cannot fix it (the Surround-collapse case
     above), it disables and re-enables the display adapter, forcing a full driver re-init and
     DisplayPort link re-train with the panels already awake, then re-applies your target. This is
     the step that recovers a collapsed 3x 4K span, and the mechanism is vendor-neutral.

**Silent fixing at startup** runs as an elevated logon task, about three seconds after you log in. It
polls for the graphics stack rather than waiting on a fixed delay, so it starts the moment it can
succeed. If nothing has drifted it exits in milliseconds and you never notice it.

## Using it

The window is deliberately small. Pick a **resolution** and a **refresh rate** from the two
dropdowns and press **Apply**:

- **Presets are triple-screen spans**: 7680x1440, 10320x1440, 15360x2160, 23040x2160.
- **Custom...** lets you type any width, height, and refresh rate.
- **Apply greys out** when your pick already matches what is on screen.
- **Fix now** only appears when the live display has drifted from your saved target.
- **Auto-fix at startup** registers the logon task (one admin prompt, once).
- **Hide on startup** launches the app minimised with Windows.

### It will not blank your screens

Every mode is validated with a non-destructive Windows test before anything is applied, so a
combination your monitors cannot drive is refused with a clear reason rather than attempted. If a
mode does apply, you get a **"Keep these display settings?"** prompt that **automatically reverts
after 12 seconds** - so if a screen goes dark, doing nothing brings it back.

Note that Windows can only switch to modes your GPU driver already publishes. To use a resolution the
driver has never heard of, create it as a custom resolution in the NVIDIA or AMD control panel first;
it will then appear here.

## What it works on

- **Windows 10 and 11**, 64-bit. No runtime dependencies.
- **Any GPU: NVIDIA, AMD, or Intel.** The detect-and-heal path (CCD + adapter restart) is
  vendor-neutral. The NVIDIA Surround collapse is specifically handled.
- **Triple-screen rigs** are the target, but the saved target is your whole desktop topology, so any
  layout works.

Validated on an **NVIDIA RTX 5070 Ti (driver 610.74)** triple-4K Surround rig at 11520x2160 @ 120 Hz,
and on an **AMD Radeon 880M** laptop - two very different setups, same fix.

## License and commercial use

Licensed under the [PolyForm Noncommercial License 1.0.0](LICENSE.md). It is **free for personal,
noncommercial use**.

**Commercial use is not permitted without a license from us.** That includes any use by or for a
company, organisation, venue, or business, and it includes an individual using it for their
employer's benefit. The restriction applies to modified versions too. If you want to use this
commercially, reach out first: **contact@lunis.live**.

## Install

Download the latest setup `.exe` from the [Releases](../../releases) page and run it. It installs for
the current user (no admin needed) and brings its own copy of the DM Sans typeface. After that the app
updates itself: open it any time and it offers a one-click update when a new version is out.

> First-download note: until the installer is code-signed, Windows SmartScreen may warn about an
> "unknown publisher." Click "More info" then "Run anyway." Updates are cryptographically signed and
> verified before they are installed.

## Build from source

Prereqs: Rust (MSVC toolchain), and [NSIS](https://nsis.sourceforge.io) if you want the installer.
No Node, no web toolchain.

```bash
cargo build -p sim-racing-display-fixer --release     # target/release/sim-display-fixer.exe
makensis /DVERSION=1.0.5 packaging/installer.nsi      # target/packaged/*-setup.exe
```

The display engine is a separate crate with a headless CLI, handy for testing without the UI:

```bash
cargo run -p lunis-display-core --bin displaycore -- status   # or: capture | fix | restart
```

Layout:

```
core/        the display engine (CCD capture/apply, adapter restart, mode list, profile)
app/         the Slint UI, updater, and the elevated --fix / --install-task entry points
packaging/   the NSIS installer script
```

## Auto-update and releasing

Pushing a `v*` tag runs `.github/workflows/release.yml`, which builds the app, packages the NSIS
installer, signs it, publishes a GitHub Release, and emits `latest.json` (the manifest the app polls).
One repo secret is required: `TAURI_SIGNING_PRIVATE_KEY`, the contents of the minisign private key.

The app verifies that signature against a baked-in public key **before** running any downloaded
installer, so an unsigned or tampered update is never executed. That is separate from Windows
code-signing for SmartScreen; until the installer is code-signed (for example with Azure Trusted
Signing), the first download shows an "unknown publisher" warning while auto-update still works.
