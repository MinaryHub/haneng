# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project

haneng is a **Windows Korean/English IME state indicator**: a small badge appears next to the **focused text caret** showing the current mode — blue **한** (Korean) / gray **a** (English lowercase) / orange **A** (English + Caps Lock). That is the whole product as of v0.4.0. The badge follows the focused caret regardless of mouse position, and hides when the caret can't be read.

**The dev machine is macOS but the product no longer runs here** — you cannot smoke-test the daemon locally. `cargo check`/`clippy` against the msvc target is the only local verification; behavioural claims about Windows apps must be verified by the user or by the daemon's own diagnostic log (`debug_log = on`), never asserted from reasoning alone.

**History**: the project originally shipped a full wrong-mode typo auto-corrector (dubeolsik/sebeolsik conversion engine, detection with dictionaries+bigrams, hotkey conversion, C FFI). After real-device testing the user cut all conversion features — too error-prone in practice — keeping only the badge. It also shipped macOS (`hanengd`, Accessibility API) and Linux/X11 (`hanengl`) daemons; those were cut in v0.9.0 after the macOS badge stopped appearing at all and the cost of chasing three platforms outweighed their use. Complete implementations are preserved at git tags `v0.3.2` (conversion) and `v0.8.13` (macOS/Linux daemons); PLAN.md describes the original (now historical) design. Do not resurrect either without an explicit request.

## Commands

- `cargo test` — all tests
- `cargo clippy --all-targets` — lint (keep warning-free)
- `cargo check -p haneng-windows --target x86_64-pc-windows-msvc` (and the same with `clippy … -- -D warnings`) — the only local verification of the daemon from this macOS dev machine
- `cargo run -p haneng-settings` — settings window (still builds on any OS, but ships only in the Windows package)
- Release: `scripts/release.sh <ver>` (version bump + CHANGELOG + test + commit + tag), then `git push origin main --tags` → release.yml builds the Windows MSI/zip into a draft release; publish with `gh release edit <tag> --draft=false --latest`.

## Architecture

The daemon finds the focused text caret, reads Korean/English + Caps Lock, and shows/hides a small badge (3-state `Mode`: 한 / a / A) that follows it. It never manipulates text, hooks input, or sends network traffic; only the settings app touches the network (update check, opt-in).

- **haneng-windows** (binary `hanengw`): **no input hooks at all** — a 150ms `WM_TIMER` on the badge window polls the focused caret and repositions/shows/hides (mouse-independent). `indicator.rs` renders a layered/click-through/no-activate topmost GDI badge window positioned **at the caret** (never the mouse): `ime.rs::caret_screen_rect` reads the focused control's `GUITHREADINFO.rcCaret` (+ `ClientToScreen`), gated by `caret_alive`: some apps (PowerPoint text boxes) end editing with `HideCaret` and never `DestroyCaret`, so `hwndCaret`/`rcCaret` linger and the badge would stick — the caret counts as gone once `GUI_CARETBLINKING` has been off for longer than ~2 blink periods (`GetCaretBlinkTime`), and the rule only applies to windows whose caret was seen blinking at least once (apps that keep a permanently hidden dummy caret for IME positioning must keep working); apps with no real Win32 caret (Chrome/Electron) report none → badge hidden (no mouse fallback — the user wants caret-only). `ime.rs::query_korean_mode` reads focused-control IME open status via `WM_IME_CONTROL`/`IMC_GETOPENSTATUS` + `SendMessageTimeoutW`; Caps via `GetKeyState(VK_CAPITAL)` toggle bit. `tray.rs` (tray-icon; muda items not Send → thread_local pattern). `debug.rs` writes show/hide reasoning to `%APPDATA%\haneng\debug.log` when `debug_log = on` (config) or `HANENG_DEBUG=1` — the daemon is a console-less GUI app auto-started at logon, so stderr is useless; never logs typed text (element Names are excluded on purpose). `windows_subsystem = "windows"`.
- **haneng-settings** (eframe/egui, glow backend — default wgpu breaks Windows via gpu-allocator/windows conflict): badge on/off, initial mode, opacity, appearance animation on/off, and the **update button** (`update.rs`: GitHub releases API + one-click MSI upgrade on Windows, browser open elsewhere; ureq with explicit TLS provider — Windows must select native-tls; 15s timeout; catch_unwind so the spinner can't hang). egui has no CJK glyphs — system Korean font loaded at runtime. eframe 0.35 renamed `App::update` to `App::ui`.
- **haneng-core**: legacy conversion-era library kept compiling with tests. Still used: `anim.rs` (`pulse_alpha` — the badge's attention pulse waveform, unit-tested) and `config.rs` (`key = value` config with `extras`, `Config::extra`/`set_extra`). `single_instance.rs` (unix flock) is dead since v0.9.0 — it existed for the macOS/Linux daemons, and Windows uses a named mutex in its own `main.rs`; kept compiling like the rest of the legacy code rather than deleted.
- **haneng-datagen**: dev tool regenerating core's embedded data (legacy modules only).

## Hard-won environment facts

- BSD sed (macOS) silently no-ops GNU `0,/re/` addresses — release.sh uses python for the version substitution and verifies the result.
- PowerShell 5.1 reads BOM-less scripts as ANSI — `scripts/package-windows.ps1` must stay ASCII-only; CI invokes it with `pwsh`.
- Win11's new Korean IME does not answer `IMC_GETCONVERSIONMODE`; `IMC_GETOPENSTATUS` is the query that works. Never trust an unanswered query as "english" — 0 is ambiguous.
- Cross-checking crates that pull `ring` (ureq/rustls) fails locally without a C cross-toolchain; the CI runners build them natively. That's why `haneng-settings` ureq uses native-tls on Windows.
- Every real module in haneng-windows is `#[cfg(windows)]`-gated (only a stub `main` compiles elsewhere) so the workspace still builds and tests on the macOS dev machine.
- PowerPoint ends text-box editing with `HideCaret` and never `DestroyCaret`, so `GUITHREADINFO` keeps reporting a stale caret — hence `caret_alive`. As of v0.8.13 the badge still lingered there; the next step is reading `debug.log` from the user's machine, not another guess.

## Versioning

Single version source: `[workspace.package] version` in the root Cargo.toml (all crates use `version.workspace = true`). Keep CHANGELOG.md (Keep a Changelog) updated under `[Unreleased]` as you work.
