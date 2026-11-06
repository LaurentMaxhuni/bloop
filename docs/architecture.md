# Native island MVP

Decision (2026-11-06): Iced 0.14, DirectX 12 GPU rendering, one Cargo package.
Modules separate UI, provider transport, platform integration, and SQLite storage.
No browser runtime, inference process, or continuously running animation timer.
Split crates only when independent consumers justify public interfaces.

Iced exposes transparency, undecorated windows, logical DPI-aware sizes, window
levels, positioning, and subscriptions. The first software-rendered build used
34 MiB working set but lost unchanged content during transparent cursor redraws.
Softbuffer's GDI surface does not reliably support the required alpha composition.
The GPU build fixes that observed failure; selecting DirectX 12 alone reduces
unused backend loading. Slint offers declarative animation
and software rendering but adds a DSL and licensing choices. egui favors immediate
mode; native Win32 would require substantially more layout, text, and input code.

Sources verified before implementation:
- https://docs.rs/iced/0.14.0/iced/window/settings/struct.Settings.html
- https://docs.rs/iced/0.14.0/iced/window/enum.Position.html
- https://docs.rs/crate/iced/0.14.0/features
- https://slint.dev/get-started
- https://docs.typesafe.ai/llms.txt
- https://github.com/rust-windowing/softbuffer/issues/17
- https://docs.ollama.com/api/chat
- https://docs.ollama.com/api/streaming
- https://developers.openai.com/api/reference/resources/chat

Windows risks: transparent software surfaces, taskbar suppression, per-monitor DPI,
focus activation restrictions, hotkey collisions, and tray message-pump ownership.
Keep Win32 calls inside the platform module. A dedicated blocking Windows message
loop owns the tray and global shortcut; UI receives events without polling.

Provider boundary: Ollama NDJSON and OpenAI-compatible SSE (including llama.cpp).
Bound frame/output/context sizes, distinguish failures from successful completion,
enforce connect/read/overall deadlines, and cancel by aborting the Iced task.
Remote endpoints are explicit configuration; plaintext HTTP restricted to loopback.
API keys come from BLOOP_API_KEY and are never written to SQLite or logs.

Jev considered for future intent/tool selection. Do not add a cloud dependency to
the local chat MVP. Future model proposals must pass a typed registry and code-owned
permissions; model confidence never grants permission. No shell/tool execution in
this milestone.

Performance targets are provisional. Windows overlay is the supported milestone;
Linux tray, compositor placement, and shortcuts require separate verification.
The first measured GPU baseline exceeds the provisional 50 MB target. A future
software compositor must preserve alpha correctly before replacing DirectX.
