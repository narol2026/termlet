//! Self-animating circular progress loading indicator.
//!
//! A rounded "pipe" track ring with a bright arc segment that sweeps around it
//! continuously, the classic indeterminate progress ring.
//!
//! It drives itself rather than waiting on an `anim_t` counter from the app's
//! update loop: the canvas program listens for
//! [`window::Event::RedrawRequested`] and schedules the next frame with
//! [`canvas::Action::request_redraw_at`]. No message plumbing, no subscription,
//! and it stops the moment it leaves the widget tree.

use std::time::{Duration, Instant};

use iced::widget::canvas;
use iced::{Color, Element, Event, Length, Radians, Rectangle, Renderer, Theme, mouse, window};

use crate::core::message::Message;

/// Angular width of the bright arc, in radians (~100°).
const SWEEP_ANGLE: f32 = 1.75;

/// Default time between animation ticks, in milliseconds.
const DEFAULT_TICK_DURATION_MS: u64 = 33;

/// Angular step per tick. Combined with [`DEFAULT_TICK_DURATION_MS`] this
/// completes a full revolution in just under 0.75 seconds (twice the default
/// rotation rate, at the same frame cadence).
const ANGLE_STEP: f32 = std::f32::consts::TAU * 2.0 / 45.0;

/// Animation progress stored in the widget tree.
#[derive(Default)]
struct LoadingIndicatorState {
    /// Angle (radians) of the arc's leading edge.
    angle: f32,
    /// Timestamp of the last tick. Kept as an `Option` so the first
    /// `RedrawRequested` event seeds the cadence instead of advancing
    /// immediately.
    last_tick: Option<Instant>,
}

/// Canvas program that draws one frame of the ring for a given active angle.
struct LoadingIndicatorProgram {
    /// Tint of the bright arc; the track ring fades it via alpha.
    color: Color,
    /// Alpha of the track ring.
    track_alpha: f32,
    /// Time between animation ticks.
    tick_duration: Duration,
}

impl canvas::Program<Message, Theme, Renderer> for LoadingIndicatorProgram {
    type State = LoadingIndicatorState;

    fn update(
        &self,
        state: &mut Self::State,
        event: &Event,
        _bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let Event::Window(window::Event::RedrawRequested(now)) = event else {
            return None;
        };

        if let Some(last) = state.last_tick {
            let elapsed = now.duration_since(last);
            if elapsed >= self.tick_duration {
                state.angle = (state.angle + ANGLE_STEP) % std::f32::consts::TAU;
                state.last_tick = Some(*now);
            }
        } else {
            state.last_tick = Some(*now);
        }

        // Always schedule the next frame so the ring keeps spinning until the
        // widget is removed from the tree.
        Some(canvas::Action::request_redraw_at(
            state.last_tick.unwrap_or(*now) + self.tick_duration,
        ))
    }

    fn draw(
        &self,
        state: &Self::State,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry<Renderer>> {
        // The Canvas widget re-invokes this every redraw (no geometry cache is
        // involved), so the ring advances simply by redrawing with a new angle.
        let mut frame = canvas::Frame::new(renderer, bounds.size());

        let center = frame.center();
        let radius = frame.width().min(frame.height()) / 2.0;
        // Scale the tube thickness with the widget; keep it at least 1px so it
        // stays visible at the 16px inline size.
        let stroke_width = (radius * 0.22).max(1.0);
        // Inset the ring so the rounded stroke stays inside the bounds.
        let ring_radius = radius - stroke_width / 2.0;

        // Track ring — the pipe wall, drawn faintly.
        let track = canvas::Path::circle(center, ring_radius);
        frame.stroke(
            &track,
            canvas::Stroke::default()
                .with_color(Color {
                    a: self.track_alpha,
                    ..self.color
                })
                .with_width(stroke_width),
        );

        // Bright arc — the segment flowing through the pipe, with rounded ends.
        let arc = canvas::Path::new(|b| {
            b.arc(canvas::path::Arc {
                center,
                radius: ring_radius,
                start_angle: Radians(state.angle),
                end_angle: Radians(state.angle + SWEEP_ANGLE),
            });
        });
        frame.stroke(
            &arc,
            canvas::Stroke::default()
                .with_color(self.color)
                .with_width(stroke_width)
                .with_line_cap(canvas::LineCap::Round),
        );

        vec![frame.into_geometry()]
    }
}

/// A self-animating circular progress loading indicator.
#[derive(Debug)]
pub struct LoadingIndicator {
    size: f32,
    color: Color,
    track_alpha: f32,
    tick_duration_ms: u64,
}

/// Create a self-animating loading indicator of the given pixel `size`, tinted
/// with `color` (typically the theme accent or a secondary text color).
///
/// The indicator animates on its own — nothing needs to drive it from the
/// application update loop:
///
/// ```text
/// let indicator: Element<'static, Message> =
///     loading_indicator(24.0, p.accent).into();
/// ```
pub fn loading_indicator(size: f32, color: Color) -> LoadingIndicator {
    LoadingIndicator {
        size,
        color,
        track_alpha: 0.15,
        tick_duration_ms: DEFAULT_TICK_DURATION_MS,
    }
}

impl LoadingIndicator {
    /// Set the animation speed as the tick interval in milliseconds. Lower
    /// values spin faster; the default is `33`.
    pub fn speed(mut self, tick_duration_ms: u64) -> Self {
        self.tick_duration_ms = tick_duration_ms;
        self
    }

    /// Fade the track ring harder (`0.06` vs `0.15` base alpha). Pass `true`
    /// on dark backgrounds so the faint track doesn't glow.
    pub fn lighter_inactive(mut self, lighter: bool) -> Self {
        self.track_alpha = if lighter { 0.06 } else { 0.15 };
        self
    }
}

impl From<LoadingIndicator> for Element<'static, Message> {
    fn from(indicator: LoadingIndicator) -> Self {
        canvas(LoadingIndicatorProgram {
            color: indicator.color,
            track_alpha: indicator.track_alpha,
            tick_duration: Duration::from_millis(indicator.tick_duration_ms),
        })
        .width(Length::Fixed(indicator.size))
        .height(Length::Fixed(indicator.size))
        .into()
    }
}
