//! Selecting Iced's own software renderer (tiny-skia) as an opt-in.

/// Maps the `BLOOP_RENDERER` opt-in onto Iced 0.14's native backend choice and
/// reads it once, here, before any window or renderer exists.
///
/// Iced compiles both backends when the `iced` `tiny-skia` feature is on and
/// picks one per process in `iced_renderer::fallback::Compositor::with_backend`,
/// which consults `ICED_BACKEND`. `BLOOP_RENDERER=software` therefore sets
/// `ICED_BACKEND=tiny-skia`; an explicit `ICED_BACKEND` from the environment
/// always wins, and any other value (or none) leaves the default — the wgpu
/// backend — in place. A settings toggle is a follow-up idea.
pub fn apply_choice() {
    if std::env::var_os("ICED_BACKEND").is_some() {
        return;
    }
    if std::env::var("BLOOP_RENDERER").as_deref() == Ok("software") {
        // SAFETY: called first in `main`, before the Iced runtime or any other
        // thread reads the environment; the process owns it.
        unsafe { std::env::set_var("ICED_BACKEND", "tiny-skia") };
    }
}

#[cfg(test)]
mod tests {
    use super::apply_choice;

    #[test]
    fn only_software_selects_tiny_skia() {
        // The only value that opts in is `software`.
        for value in ["gpu", "", "TINY-SKIA", "tiny-skia"] {
            // SAFETY: test; sequential process-global edits in the same test.
            unsafe {
                std::env::set_var("BLOOP_RENDERER", value);
                std::env::remove_var("ICED_BACKEND");
            }
            apply_choice();
            assert_eq!(std::env::var_os("ICED_BACKEND"), None);
        }
        // SAFETY: test cleanup.
        unsafe {
            std::env::set_var("BLOOP_RENDERER", "software");
            std::env::remove_var("ICED_BACKEND");
        }
        apply_choice();
        assert_eq!(std::env::var_os("ICED_BACKEND"), Some("tiny-skia".into()));
    }
}
