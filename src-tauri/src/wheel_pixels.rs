// Linux, mouse wheel: WebKitGTK scrolls `ticks × step` pixels per wheel event, with
// step = (view height)^(2/3) truncated to a whole number (WebCore's
// Scrollbar::pixelsPerLineStep(int) on GTK). KWin reports one wheel click as 15, which
// GTK divides by 10, so a click is 1.5 ticks. When the step is odd, every click scrolls
// by n.5 pixels: every other click leaves the page on a half pixel, and WebKit's GPU
// compositor draws the scrolled text blurred.
//
// Before WebKit's own handler runs, change the event's deltas so that WebKit's
// multiplication gives a whole number of pixels. What is rounded away is carried over
// to the next event, so the total distance stays the same (a 139.5 px click becomes
// 140, 139, 140, ... px).
//
// HOSHI_WHEEL_SNAP=0 turns this off, HOSHI_WHEEL_DEBUG=1 prints every wheel event.

use std::cell::Cell;

use gtk::gdk;
use gtk::glib::translate::ToGlibPtr;
use gtk::glib::Propagation;
use gtk::prelude::*;
use tauri::{Runtime, WebviewWindow};

/// WebCore `Scrollbar::pixelsPerLineStep(int)` on GTK: `std::pow(size, 2. / 3.)`
/// converted to int (truncated), or 40 if the size is unknown.
fn line_step(size: i32) -> i32 {
    if size > 0 {
        (size as f64).powf(2.0 / 3.0) as i32
    } else {
        40
    }
}

/// The delta to put in the event for one axis, given the GTK delta, WebKit's step for
/// that axis and the carried-over rounding.
///
/// WebKit converts the delta to float and multiplies it by the step as float, so the
/// result is the whole number of pixels or the float next to it: a few millionths of a
/// pixel off, far below what the GPU can show. That difference is carried over too, so
/// it never adds up.
fn snap(delta: f64, step: i32, carry: &Cell<f64>) -> f64 {
    if delta == 0.0 {
        return 0.0;
    }
    let exact = delta * step as f64 + carry.get();
    let pixels = exact.round();
    if pixels == 0.0 {
        carry.set(exact);
        return 0.0;
    }
    let ticks = (pixels / step as f64) as f32;
    // what WebKit will scroll
    let scrolled = ticks * step as f32;
    carry.set(exact - scrolled as f64);
    ticks as f64
}

pub fn install<R: Runtime>(window: &WebviewWindow<R>) {
    let enabled = std::env::var("HOSHI_WHEEL_SNAP").map_or(true, |value| value != "0");
    let debug = std::env::var("HOSHI_WHEEL_DEBUG").is_ok_and(|value| value == "1");
    if debug {
        eprintln!("hoshi wheel: whole-pixel wheel scrolling {}", if enabled { "on" } else { "off" });
    }
    if !enabled && !debug {
        return;
    }
    let result = window.with_webview(move |webview| {
        attach(webview.inner().upcast_ref(), enabled, debug);
    });
    if let Err(error) = result {
        eprintln!("hoshi wheel: could not reach the webview: {error}");
    }
}

/// Connects the handler to the WebKitWebView widget; it runs before WebKit's own.
fn attach(widget: &gtk::Widget, enabled: bool, debug: bool) {
    let carry = (Cell::new(0.0), Cell::new(0.0));
    widget.connect_scroll_event(move |widget, event| {
        // WebKit uses the view-size step only for smooth events from a mouse
        // (touchpads get a fixed step of 40 and are left alone)
        let is_mouse = event
            .source_device()
            .is_some_and(|device| device.source() == gdk::InputSource::Mouse);
        if event.direction() != gdk::ScrollDirection::Smooth || !is_mouse {
            return Propagation::Proceed;
        }
        let (dx, dy) = event.delta();
        // the deltas are multiplied by the step of their own axis, also when Shift
        // swaps the scroll direction
        let step_x = line_step(widget.allocated_width());
        let step_y = line_step(widget.allocated_height());
        let snapped =
            enabled.then(|| (snap(dx, step_x, &carry.0), snap(dy, step_y, &carry.1)));
        if debug {
            let (sx, sy) = snapped.unwrap_or((dx, dy));
            eprintln!(
                "hoshi wheel: view {}x{}, step {}x{}, delta ({dx}, {dy}) = ({} px, {} px) -> sent ({sx}, {sy}) = ({} px, {} px)",
                widget.allocated_width(),
                widget.allocated_height(),
                step_x,
                step_y,
                dx as f32 * step_x as f32,
                dy as f32 * step_y as f32,
                sx as f32 * step_x as f32,
                sy as f32 * step_y as f32,
            );
        }
        let Some((sx, sy)) = snapped else {
            return Propagation::Proceed;
        };
        if sx == 0.0 && sy == 0.0 && !event.is_stop() {
            // less than half a pixel so far: it is carried over to the next event
            return Propagation::Stop;
        }
        // GTK passes this same event on to WebKit's handler (the signal's event
        // is static scope, not a copy), so write the new deltas into it
        let raw =
            <gdk::EventScroll as ToGlibPtr<*const gdk::ffi::GdkEventScroll>>::to_glib_none(event).0;
        unsafe {
            let raw = raw as *mut gdk::ffi::GdkEventScroll;
            (*raw).delta_x = sx;
            (*raw).delta_y = sy;
        }
        Propagation::Proceed
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_matches_webkit() {
        assert_eq!(line_step(900), 93);
        assert_eq!(line_step(1000), 99);
        assert_eq!(line_step(1036), 102);
        assert_eq!(line_step(0), 40);
    }

    #[test]
    fn kwin_clicks_land_on_whole_pixels() {
        // every window size from the minimum (640×480) to 8K, on both axes
        for size in 480..=4320 {
            let step = line_step(size);
            let carry = Cell::new(0.0);
            let mut position = 0.0f32;
            let mut ticks = 0.0;
            for click in 0..400 {
                // mostly single clicks, sometimes two in one event, sometimes back up
                let delta = match click % 9 {
                    4 => 3.0,
                    7 => -1.5,
                    _ => 1.5,
                };
                ticks += delta;
                let sent = snap(delta, step, &carry);
                // what WebKit does: float(-delta) × float(step), added to a float offset
                position += (-(sent as f32)) * step as f32;
                let off = (position - position.round()).abs();
                assert!(off < 0.001, "size {size}, step {step}, click {click}: {position}");
            }
            let expected = -ticks * step as f64;
            assert!((position as f64 - expected).abs() <= 0.5, "size {size}: {position} vs {expected}");
        }
    }

    #[test]
    fn odd_steps_alternate_instead_of_half_pixels() {
        // a 900 px view: step 93, 139.5 px per click
        let carry = Cell::new(0.0);
        let clicks: Vec<f32> = (0..4).map(|_| snap(1.5, 93, &carry) as f32 * 93.0).collect();
        for (click, expected) in clicks.iter().zip([140.0, 139.0, 140.0, 139.0]) {
            assert!((click - expected).abs() < 0.001, "{clicks:?}");
        }
    }

    #[test]
    fn small_deltas_are_carried_over() {
        let carry = Cell::new(0.0);
        let step = 93;
        let mut total = 0.0;
        for _ in 0..40 {
            total += snap(0.0125, step, &carry) * step as f64;
        }
        assert!((total - 0.0125 * 40.0 * 93.0).abs() <= 0.5 + 1e-6);
    }
}
