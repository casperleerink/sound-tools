//! A plain `NSView` standing in for a plugin's editor: it draws, takes the keyboard when
//! clicked, logs every mouse and key event, uses the key `a` and passes every other key on up
//! the responder chain, as a plugin editor does.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSBezierPath, NSColor, NSEvent, NSStringDrawing, NSView};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSSize, NSString};

use crate::log;

#[derive(Default)]
pub struct Ivars {
    last: RefCell<String>,
    clicks: Cell<u32>,
    focused: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "RuneSpikeEditorView"]
    #[ivars = Ivars]
    pub struct EditorView;

    impl EditorView {
        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(becomeFirstResponder))]
        fn become_first_responder(&self) -> bool {
            self.ivars().focused.set(true);
            log("VIEW became first responder");
            self.setNeedsDisplay(true);
            true
        }

        #[unsafe(method(resignFirstResponder))]
        fn resign_first_responder(&self) -> bool {
            self.ivars().focused.set(false);
            log("VIEW resigned first responder");
            self.setNeedsDisplay(true);
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            self.ivars().clicks.set(self.ivars().clicks.get() + 1);
            self.note(format!("mouse down at ({:.0}, {:.0}), click {}", at.x, at.y, self.ivars().clicks.get()));
            if let Some(window) = self.window() {
                window.makeFirstResponder(Some(self));
            }
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let at = self.convertPoint_fromView(event.locationInWindow(), None);
            self.note(format!("mouse dragged to ({:.0}, {:.0})", at.x, at.y));
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, _event: &NSEvent) {
            self.note("mouse up".into());
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            self.note(format!("scroll {:.1}", event.scrollingDeltaY()));
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let characters = event.charactersIgnoringModifiers().map(|c| c.to_string()).unwrap_or_default();
            if characters == "a" {
                self.note(format!("key down {characters:?}: TAKEN by the view"));
            } else {
                self.note(format!("key down {characters:?} (code {}): passed up the responder chain", event.keyCode()));
                // SAFETY: NSResponder's own keyDown: sends the event to the next responder.
                unsafe { msg_send![super(self), keyDown: event] }
            }
        }

        #[unsafe(method(keyUp:))]
        fn key_up(&self, event: &NSEvent) {
            let characters = event.charactersIgnoringModifiers().map(|c| c.to_string()).unwrap_or_default();
            if characters == "a" {
                self.note(format!("key up {characters:?}: TAKEN"));
            } else {
                self.note(format!("key up {characters:?}: passed up"));
                // SAFETY: as in keyDown:.
                unsafe { msg_send![super(self), keyUp: event] }
            }
        }

        #[unsafe(method(drawRect:))]
        fn draw_rect(&self, _dirty: NSRect) {
            let bounds = self.bounds();
            let ivars = self.ivars();
            let background = match ivars.focused.get() {
                true => NSColor::colorWithSRGBRed_green_blue_alpha(1.0, 0.95, 0.7, 1.0),
                false => NSColor::colorWithSRGBRed_green_blue_alpha(0.85, 0.88, 0.92, 1.0),
            };
            background.setFill();
            NSBezierPath::fillRect(bounds);
            // Sharpness: 1 pt bars and 0.5 pt hairlines. On a 2x display a hairline is one
            // device pixel; drawn at the wrong scale it blurs to grey.
            NSColor::blackColor().setFill();
            for i in 0..20 {
                let x = 16.0 + f64::from(i) * 2.0;
                NSBezierPath::fillRect(NSRect::new(NSPoint::new(x, 110.0), NSSize::new(1.0, 40.0)));
                let x = 72.0 + f64::from(i);
                NSBezierPath::fillRect(NSRect::new(NSPoint::new(x, 110.0), NSSize::new(0.5, 40.0)));
            }
            // A 1 pt border, so a capture shows exactly where the view ends.
            NSColor::redColor().setFill();
            for edge in [
                NSRect::new(bounds.origin, NSSize::new(bounds.size.width, 1.0)),
                NSRect::new(NSPoint::new(0.0, bounds.size.height - 1.0), NSSize::new(bounds.size.width, 1.0)),
                NSRect::new(bounds.origin, NSSize::new(1.0, bounds.size.height)),
                NSRect::new(NSPoint::new(bounds.size.width - 1.0, 0.0), NSSize::new(1.0, bounds.size.height)),
            ] {
                NSBezierPath::fillRect(edge);
            }
            let scale = self.window().map_or(0.0, |w| w.backingScaleFactor());
            let lines = [
                "Stand-in editor: a plain NSView".to_string(),
                format!("first responder: {}   backing scale: {scale}", ivars.focused.get()),
                format!("size: {:.0} x {:.0} pt", bounds.size.width, bounds.size.height),
                format!("last: {}", ivars.last.borrow()),
            ];
            for (i, line) in lines.iter().enumerate() {
                let point = NSPoint::new(16.0, 14.0 + 20.0 * i as f64);
                // SAFETY: no attributes, so AppKit's defaults.
                unsafe { NSString::from_str(line).drawAtPoint_withAttributes(point, None) };
            }
        }
    }
);

impl EditorView {
    fn note(&self, text: String) {
        log(format!("VIEW {text}"));
        *self.ivars().last.borrow_mut() = text;
        self.setNeedsDisplay(true);
    }
}

fn parent_view<'a>(view: NonNull<c_void>) -> &'a NSView {
    // SAFETY: the raw handle's `ns_view` is the window's live content view, used on the main
    // thread inside the window's life.
    unsafe { view.cast::<NSView>().as_ref() }
}

/// The editor at the top of `parent`, filling all of it but the strip Rune draws at the bottom.
/// It follows the window as it resizes, as a resizable plugin's editor would.
pub fn attach(parent: NonNull<c_void>, size: (f64, f64)) -> Retained<EditorView> {
    let parent = parent_view(parent);
    let Some(mtm) = MainThreadMarker::new() else { panic!("attach runs on the main thread") };
    let bounds = parent.bounds();
    // Rune's view (winit's) is flipped: y counts down from the top.
    let top = if parent.isFlipped() { 0.0 } else { bounds.size.height - size.1 };
    let frame = NSRect::new(NSPoint::new(0.0, top), NSSize::new(size.0, size.1));
    let this = mtm.alloc::<EditorView>().set_ivars(Ivars::default());
    // SAFETY: NSView's designated initializer.
    let view: Retained<EditorView> = unsafe { msg_send![super(this), initWithFrame: frame] };
    view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable);
    parent.addSubview(&view);
    log(format!("stand-in attached at {}", frame_of(&view)));
    view
}

pub fn frame_of(view: &NSView) -> String {
    let f = view.frame();
    format!("frame ({:.0}, {:.0}) {:.0}x{:.0} pt", f.origin.x, f.origin.y, f.size.width, f.size.height)
}

fn class_of(object: &AnyObject) -> String {
    object.class().name().to_string_lossy().into_owned()
}

fn layers_of(view: &NSView) -> String {
    // SAFETY: `layer` and `sublayers` are plain getters on a layer-backed view.
    let layer: Option<Retained<NSObject>> = unsafe { msg_send![view, layer] };
    let Some(layer) = layer else { return "no layer".into() };
    let sublayers: Option<Retained<NSArray<NSObject>>> = unsafe { msg_send![&*layer, sublayers] };
    let names = sublayers.map(|list| list.iter().map(|l| class_of(&l)).collect::<Vec<_>>().join(", ")).unwrap_or_default();
    format!("layer {} with sublayers [{names}] (back to front)", class_of(&layer))
}

pub fn describe_parent(view: NonNull<c_void>) -> String {
    let view = parent_view(view);
    format!("{} wantsLayer {}, {}", class_of(view), view.wantsLayer(), layers_of(view))
}

pub fn describe_subviews(view: NonNull<c_void>) -> String {
    let view = parent_view(view);
    let subviews = view.subviews().iter().map(|v| format!("{} {}", class_of(&v), frame_of(&v))).collect::<Vec<_>>().join("; ");
    format!("subviews [{subviews}]; {}", layers_of(view))
}

/// What GPUI's `WindowKind::Floating` gave plugin-host: above the app's other windows, and
/// hidden while another application is active. Rune has no option for either, but its window
/// is a plain NSWindow behind the raw handle.
pub fn float_above_main(view: NonNull<c_void>) {
    let Some(window) = parent_view(view).window() else { return };
    // NSFloatingWindowLevel.
    window.setLevel(3);
    // Off for unattended captures while another app is in front.
    window.setHidesOnDeactivate(std::env::var_os("SPIKE_KEEP_VISIBLE").is_none());
}

/// The class of the view that has the keyboard in `view`'s window.
pub fn first_responder(view: NonNull<c_void>) -> String {
    let Some(window) = parent_view(view).window() else { return "no window".into() };
    window.firstResponder().map_or("none".into(), |responder| class_of(&responder))
}
