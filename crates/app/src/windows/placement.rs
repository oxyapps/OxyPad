//! Where each window was on screen, so it opens there again.
//!
//! A saved place is only used while it still fits on its display. A window
//! last seen on an external monitor that is gone, or on a screen that has
//! since become smaller, opens at the default place instead, never off screen.

use gpui_kit::*;
use oxypad_session::WindowPlacement;

/// The smallest a window can be made.
pub const MIN_SIZE: Size<Pixels> = Size { width: px(480.), height: px(320.) };

/// How far a saved window may reach past the edges of its display and still
/// count as fitting: Windows reports a window snapped to an edge a few
/// pixels beyond it.
const SLACK: Pixels = px(16.);

/// Where `window` is now.
pub fn current(window: &Window, cx: &App) -> WindowPlacement {
    let (bounds, maximized) = match window.window_bounds() {
        WindowBounds::Windowed(bounds) | WindowBounds::Fullscreen(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) => (bounds, true),
    };
    WindowPlacement {
        display: window.display(cx).and_then(|display| display.uuid().ok()).map(|uuid| uuid.to_string()),
        x: f32::from(bounds.origin.x).round() as i32,
        y: f32::from(bounds.origin.y).round() as i32,
        width: f32::from(bounds.size.width).round() as i32,
        height: f32::from(bounds.size.height).round() as i32,
        maximized,
    }
}

/// The bounds and display to open a window at `placement`, or `None` when it
/// no longer fits on any display.
pub fn restore(placement: &WindowPlacement, cx: &App) -> Option<(WindowBounds, DisplayId)> {
    let bounds = Bounds {
        origin: point(px(placement.x as f32), px(placement.y as f32)),
        size: size(px(placement.width as f32), px(placement.height as f32)),
    };
    let displays = cx.displays();
    // The display it was on; without a stable id, the first it fits on.
    let display = match &placement.display {
        Some(id) => displays
            .iter()
            .find(|display| display.uuid().is_ok_and(|uuid| uuid.to_string() == *id))?,
        None => displays.iter().find(|display| fits(bounds, display.bounds()))?,
    };
    if !fits(bounds, display.bounds()) {
        return None;
    }
    let bounds = if placement.maximized { WindowBounds::Maximized(bounds) } else { WindowBounds::Windowed(bounds) };
    Some((bounds, display.id()))
}

/// Whether a window at `bounds` is whole on a display at `display`, and
/// not smaller than a window can be.
fn fits(bounds: Bounds<Pixels>, display: Bounds<Pixels>) -> bool {
    bounds.size.width >= MIN_SIZE.width
        && bounds.size.height >= MIN_SIZE.height
        && bounds.left() >= display.left() - SLACK
        && bounds.top() >= display.top() - SLACK
        && bounds.right() <= display.right() + SLACK
        && bounds.bottom() <= display.bottom() + SLACK
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Bounds, Pixels, point, px, size};

    use super::fits;

    fn rect(x: f32, y: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds { origin: point(px(x), px(y)), size: size(px(width), px(height)) }
    }

    fn laptop() -> Bounds<Pixels> {
        rect(0., 0., 1512., 982.)
    }

    #[test]
    fn a_window_inside_its_display_fits() {
        assert!(fits(rect(100., 80., 1100., 720.), laptop()));
    }

    #[test]
    fn a_window_snapped_to_an_edge_fits() {
        assert!(fits(rect(-7., 0., 756., 982.), laptop()));
    }

    #[test]
    fn a_window_bigger_than_the_display_does_not_fit() {
        // Last used on a 4K monitor, now on the laptop screen.
        assert!(!fits(rect(0., 0., 2400., 1300.), laptop()));
    }

    #[test]
    fn a_window_off_the_display_does_not_fit() {
        // Was on a monitor to the right of the laptop.
        assert!(!fits(rect(1700., 100., 1100., 720.), laptop()));
        assert!(!fits(rect(1000., 100., 1100., 720.), laptop()));
    }

    #[test]
    fn a_display_left_of_the_main_one_has_negative_coordinates() {
        assert!(fits(rect(-1800., 120., 1100., 720.), rect(-1920., 0., 1920., 1080.)));
    }

    #[test]
    fn a_window_below_the_minimum_size_does_not_fit() {
        assert!(!fits(rect(10., 10., 200., 100.), laptop()));
    }
}
