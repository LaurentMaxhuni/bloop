# MVP verification — 2026-11-06

Windows x64, Rust 1.97.1, Iced 0.14, native MSVC toolchain.

Passed on final source:
- `cargo fmt --check`
- `cargo clippy --all-targets -- -D warnings`
- `cargo test`: 19 passed (10 library, 5 UI state, 4 real HTTP transport tests).
- `cargo build --release`

An independent source review found nullable tool fields being rejected and saves
that could exceed reload limits. Both received failing regression tests and fixes;
the complete suite then passed. Shutdown, animation, stale-request handling, and
saved-chat clearing are also covered by state tests.

Real Windows UI checks: compact and expanded transparency, stable cursor redraw,
default and configured global shortcuts, dark/light theme, settings layout, token
streaming from a labeled local fixture, Stop, history/theme restoration after
restart, and quit through settings and Alt+F4. Fixture used isolated local data;
no real conversation was sent to a cloud service. No local model server was running,
so actual inference quality and model-specific compatibility remain unverified.
The tray message loop initialized without errors; tray menu selection itself has
not been manually exercised.

Performance observations (single process, external inference excluded):

| Build / state | Working set | Private bytes | Observation |
| --- | --- | --- | --- |
| Initial tiny-skia | ~34 MiB | ~11 MiB | Transparent partial redraw failed; rejected |
| All GPU backends, compact | 187.13 MiB | 200.58 MiB | 93.75 ms CPU over 8.05 s |
| DirectX-only final release, compact | 100.52 MiB | 107.16 MiB | 0 ms CPU delta over 8.07 s |

Final release executable: 14.28 MiB. Final sample used normal default settings,
20 process threads (including graphics-driver threads), and no backend override.
Tokio is explicitly limited to two async workers and two blocking workers.
These short samples are observations, not guarantees or a full profiling study.
Working-set and private-memory metrics differ; neither includes dedicated GPU VRAM.
The sub-50 MB target is not achieved. Startup latency, 60 FPS cadence, GPU energy
use, and mixed-DPI/multi-monitor hotplug behavior have not been benchmarked.

Known scope limits: Windows/DirectX 12 only; primary-monitor anchoring; incomplete
screen-reader integration; one stored conversation and one instance per data
directory; no OS tools or autonomous agent loop. Phase 3 remains separate work.

Sandboxed MSVC archive writes and network access were blocked. Cargo dependency
fetches, native builds/tests, and interactive launches were run with approved
environment access. No changes to global Rust, Git, or Windows configuration.

## Auto-hide notch (2026-11-06)

Checked on Windows 11 (100 % scale) with a debug build and a scratch `BLOOP_DATA_DIR`,
using `WindowFromPoint` to see which process owns each screen point:

| Check | Result |
| --- | --- |
| At rest only the sliver belongs to bloop; points 70 px right of it and 20 px below it belong to the window underneath | Pass |
| Resting the cursor on the sliver for 0.7 s shows the pill with the mascot | Pass |
| Moving away shrinks back to the sliver within 1.2 s | Pass |
| Clicking the pill opens the full panel | Pass |
| Activating another window closes the panel | Pass |
| Settings › Peek test (debug only) shows "Hi! This is a peek." and returns to the sliver by 5 s | Pass |
| A borderless fullscreen top-most window hides the sliver within the 2 s poll; it returns after the window closes | Pass |

Not checked: a real fullscreen game or video, the lock screen, mixed-DPI monitors,
and a reply finishing while the panel is closed (covered by unit tests only).
