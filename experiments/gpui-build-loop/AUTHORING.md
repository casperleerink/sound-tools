# GPUI custom view authoring experiment

Started reading official docs approximately 2026-09-10 04:03 UTC.
First complete source written at 04:04:31 UTC, about 1.5 minutes of reading and authoring. Cargo fmt completed without errors. Build wait is recorded separately from authoring time.
Task: two independent custom editor instances with four clickable steps and level controls. No audio, persistence, framework, or SDK.

Sources read before authoring:
- https://docs.rs/gpui/0.2.2/gpui/
- https://gpui.rs/ (Hello World example)
- https://docs.rs/gpui/0.2.2/gpui/struct.Context.html
- https://docs.rs/gpui/0.2.2/gpui/trait.StatefulInteractiveElement.html

An attempt is a source revision submitted to Cargo for compilation. Dependency download or platform-toolchain failures before checking this source do not count as a source attempt, and must be reported separately. Formatting does not count. UI fixes after a successful compile would count as additional source attempts if rebuilt.

Source revision 1: independent Entity<PulseEditor> state, per-entity listener closures and notify calls, repeated local element IDs under separate entities.
Compilation delegated to timing experiment owner to avoid concurrent Cargo cache contention.
Attempt 1 passed with no custom-view source fixes. The timing owner reports 2.151 seconds for the integrated extension and runtime build plus link, with 0.370 seconds in the linker. The owner added a REVISION constant and its visible heading for timing instrumentation.

The host runtime, authored separately, needed one correction to remove unsupported WindowOptions.title and import prelude/AppContext. This is separate from custom-view authoring and must not be hidden in the overall experiment report.

UI verification passed through native screenshot-based clicks. A step 1 was disabled and its level raised to 60%; B stayed at its original state. Then B step 2 was enabled and its level lowered to 40%; A retained its edits. See results/independent-editors.png. The accessibility tree exposed only the window controls, not the custom editor controls. No API difficulties were encountered in the bounded custom view. This does not establish how well an agent handles text input, drag gestures, audio visualization, accessibility, or a larger editor.

The timing experiment's first baseline build failed in GPUI build.rs because the Xcode Metal Toolchain is missing. No custom-view source had compiled. The owner enabled GPUI's runtime_shaders feature and retried. This is a platform setup failure, separate from the view authoring attempt count. The standalone manifest matches that feature choice.
