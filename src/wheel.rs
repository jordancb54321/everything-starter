use std::f32::consts::TAU;

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Ui, Vec2};

use crate::dashboard::{
    accent_green, border_color, panel_alt_fill, panel_fill, text_primary, workspace_fill,
};

#[derive(Clone, Copy, Debug)]
pub(crate) struct SpinAnimation {
    pub(crate) start_time: f64,
    pub(crate) duration: f64,
    pub(crate) start_rotation: f32,
    pub(crate) end_rotation: f32,
    pub(crate) target: WheelTarget,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WheelTarget {
    Daily(u64),
    Recurring(u64),
}

#[derive(Clone, Debug)]
pub(crate) struct WheelEntry<'a> {
    pub(crate) target: WheelTarget,
    pub(crate) title: &'a str,
    pub(crate) weight: f64,
}

pub(crate) struct WheelState {
    pub(crate) rotation: f32,
    pub(crate) spin: Option<SpinAnimation>,
    pub(crate) rng_state: u64,
    pub(crate) include_recurring: bool,
}

impl WheelState {
    pub(crate) fn new(rng_state: u64, include_recurring: bool) -> Self {
        Self {
            rotation: 0.0,
            spin: None,
            rng_state,
            include_recurring,
        }
    }
}

pub(crate) fn weighted_choice_index(weights: &[f64], unit_random: f64) -> usize {
    let total = weights
        .iter()
        .copied()
        .filter(|weight| *weight > 0.0)
        .sum::<f64>();
    if total <= f64::EPSILON {
        return 0;
    }

    let mut threshold = unit_random.clamp(0.0, 1.0 - f64::EPSILON) * total;
    for (index, weight) in weights.iter().copied().enumerate() {
        if weight <= 0.0 {
            continue;
        }
        if threshold < weight {
            return index;
        }
        threshold -= weight;
    }

    weights.len().saturating_sub(1)
}

pub(crate) fn draw_wheel(
    ui: &Ui,
    rect: Rect,
    entries: &[WheelEntry],
    rotation: f32,
    selected_target: Option<WheelTarget>,
) {
    let painter = ui.painter_at(rect);
    let center = rect.center();
    let radius = rect.width().min(rect.height()) * 0.44;

    painter.circle_filled(center, radius + 8.0, Color32::from_rgb(10, 16, 26));
    painter.circle_stroke(center, radius + 8.0, Stroke::new(1.0, border_color()));

    if entries.is_empty() {
        painter.circle_filled(center, radius, panel_alt_fill());
        painter.text(
            center,
            Align2::CENTER_CENTER,
            "Done",
            FontId::proportional(30.0),
            accent_green(),
        );
        draw_pointer(&painter, center, radius);
        return;
    }

    let segment_angle = TAU / entries.len() as f32;
    let colors = [
        Color32::from_rgb(59, 130, 246),
        Color32::from_rgb(16, 185, 129),
        Color32::from_rgb(245, 158, 11),
        Color32::from_rgb(20, 184, 166),
        Color32::from_rgb(239, 68, 68),
        Color32::from_rgb(99, 102, 241),
    ];

    if entries.len() == 1 {
        let entry = &entries[0];
        let fill = if selected_target == Some(entry.target) {
            lighten(colors[0], 0.18)
        } else {
            colors[0]
        };

        painter.circle_filled(center, radius, fill);
        painter.circle_stroke(center, radius, Stroke::new(1.4, workspace_fill()));
        draw_centered_wheel_label(
            &painter,
            Pos2::new(center.x, center.y + radius * 0.38),
            entry.title,
            radius * 1.35,
            18.0,
            12.0,
        );
        painter.circle_filled(center, radius * 0.18, panel_fill());
        painter.circle_stroke(center, radius * 0.18, Stroke::new(2.0, border_color()));
        draw_pointer(&painter, center, radius);
        return;
    }

    for (index, entry) in entries.iter().enumerate() {
        let start = rotation + index as f32 * segment_angle;
        let end = start + segment_angle;
        let color = colors[index % colors.len()];
        let fill = if selected_target == Some(entry.target) {
            lighten(color, 0.18)
        } else {
            color
        };

        let mut points = Vec::with_capacity(28);
        points.push(center);
        let steps = 24;
        for step in 0..=steps {
            let t = start + (end - start) * step as f32 / steps as f32;
            points.push(Pos2::new(
                center.x + t.cos() * radius,
                center.y + t.sin() * radius,
            ));
        }

        painter.add(Shape::convex_polygon(
            points,
            fill,
            Stroke::new(1.4, workspace_fill()),
        ));
    }

    for (index, entry) in entries.iter().enumerate() {
        let start = rotation + index as f32 * segment_angle;
        let label_angle = start + segment_angle * 0.5;
        if entries.len() < 5 {
            let label_radius = radius * 0.58;
            let label_pos = Pos2::new(
                center.x + label_angle.cos() * label_radius,
                center.y + label_angle.sin() * label_radius,
            );
            let max_width =
                (2.0 * label_radius * (segment_angle * 0.5).sin() - 12.0).max(radius * 0.35);
            draw_centered_wheel_label(&painter, label_pos, entry.title, max_width, 16.0, 10.0);
        } else {
            draw_radial_wheel_label(
                &painter,
                center,
                radius,
                label_angle,
                segment_angle,
                entries.len(),
                entry.title,
            );
        }
    }

    painter.circle_filled(center, radius * 0.18, panel_fill());
    painter.circle_stroke(center, radius * 0.18, Stroke::new(2.0, border_color()));
    draw_pointer(&painter, center, radius);
}

fn draw_pointer(painter: &egui::Painter, center: Pos2, radius: f32) {
    let tip = Pos2::new(center.x, center.y - radius - 16.0);
    let left = Pos2::new(center.x - 18.0, center.y - radius + 26.0);
    let right = Pos2::new(center.x + 18.0, center.y - radius + 26.0);
    painter.add(Shape::convex_polygon(
        vec![tip, left, right],
        text_primary(),
        Stroke::NONE,
    ));
}

fn draw_centered_wheel_label(
    painter: &egui::Painter,
    pos: Pos2,
    title: &str,
    max_width: f32,
    preferred_font_size: f32,
    min_font_size: f32,
) {
    if let Some((label, font_id)) = fitted_wheel_label(
        painter,
        title,
        max_width,
        preferred_font_size,
        min_font_size,
    ) {
        painter.text(pos, Align2::CENTER_CENTER, label, font_id, Color32::WHITE);
    }
}

fn draw_radial_wheel_label(
    painter: &egui::Painter,
    center: Pos2,
    radius: f32,
    label_angle: f32,
    segment_angle: f32,
    entry_count: usize,
    title: &str,
) {
    let hub_radius = radius * 0.18;
    let radial_padding = (radius * 0.07).clamp(8.0, 14.0);
    let available_width = (radius - hub_radius - radial_padding * 2.0).max(0.0);
    if available_width < 18.0 {
        return;
    }

    let label_radius = hub_radius + radial_padding + available_width * 0.5;
    let tangential_width = 2.0 * label_radius * (segment_angle * 0.5).sin();
    if tangential_width < 8.0 {
        return;
    }

    let count_scale = (10.0 / entry_count as f32).sqrt().clamp(0.72, 1.0);
    let preferred_font_size = (((radius * 0.085).clamp(10.0, 16.0) * count_scale)
        .min((tangential_width * 0.74).clamp(8.0, 16.0)))
    .clamp(8.0, 16.0);

    let Some((label, font_id)) =
        fitted_wheel_label(painter, title, available_width, preferred_font_size, 8.0)
    else {
        return;
    };

    let galley = painter.layout_no_wrap(label, font_id, Color32::WHITE);
    let text_size = galley.size();
    let mut text_angle = label_angle;
    if text_angle.cos() < 0.0 {
        text_angle += std::f32::consts::PI;
    }

    let label_center = Pos2::new(
        center.x + label_angle.cos() * label_radius,
        center.y + label_angle.sin() * label_radius,
    );
    let center_offset = rotated_vec2(text_size * 0.5, text_angle);
    let text_pos = label_center - center_offset;

    painter
        .add(egui::epaint::TextShape::new(text_pos, galley, Color32::WHITE).with_angle(text_angle));
}

fn fitted_wheel_label(
    painter: &egui::Painter,
    title: &str,
    max_width: f32,
    preferred_font_size: f32,
    min_font_size: f32,
) -> Option<(String, FontId)> {
    if title.is_empty() || max_width <= 0.0 {
        return None;
    }

    let min_font_size = min_font_size.min(preferred_font_size);
    let mut font_size = preferred_font_size;
    while font_size > min_font_size {
        let font_id = FontId::proportional(font_size);
        if wheel_label_width(painter, title, font_id.clone()) <= max_width {
            return Some((title.to_owned(), font_id));
        }
        font_size -= 1.0;
    }

    let font_id = FontId::proportional(min_font_size);
    if wheel_label_width(painter, title, font_id.clone()) <= max_width {
        return Some((title.to_owned(), font_id));
    }

    ellipsized_wheel_label(painter, title, font_id.clone(), max_width).map(|label| (label, font_id))
}

fn ellipsized_wheel_label(
    painter: &egui::Painter,
    title: &str,
    font_id: FontId,
    max_width: f32,
) -> Option<String> {
    let chars: Vec<char> = title.chars().collect();
    if chars.is_empty() {
        return None;
    }

    let mut low = 1;
    let mut high = chars.len();
    let mut best = 0;

    while low <= high {
        let mid = (low + high) / 2;
        let candidate = format!("{}...", chars[..mid].iter().collect::<String>());
        if wheel_label_width(painter, &candidate, font_id.clone()) <= max_width {
            best = mid;
            low = mid + 1;
        } else {
            high = mid - 1;
        }
    }

    if best > 0 {
        Some(format!("{}...", chars[..best].iter().collect::<String>()))
    } else {
        let first = chars[0].to_string();
        (wheel_label_width(painter, &first, font_id) <= max_width).then_some(first)
    }
}

fn wheel_label_width(painter: &egui::Painter, text: &str, font_id: FontId) -> f32 {
    painter
        .layout_no_wrap(text.to_owned(), font_id, Color32::WHITE)
        .size()
        .x
}

fn rotated_vec2(vec: Vec2, angle: f32) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    Vec2::new(cos * vec.x - sin * vec.y, sin * vec.x + cos * vec.y)
}

pub(crate) fn fit_row_label(title: &str) -> String {
    let mut chars = title.chars();
    let mut result = String::new();
    for _ in 0..36 {
        let Some(ch) = chars.next() else {
            return title.to_owned();
        };
        result.push(ch);
    }

    if chars.next().is_some() {
        result.push_str("...");
    }
    result
}

fn lighten(color: Color32, amount: f32) -> Color32 {
    let mix = |channel: u8| -> u8 {
        let value = channel as f32 + (255.0 - channel as f32) * amount;
        value.round().clamp(0.0, 255.0) as u8
    };

    Color32::from_rgb(mix(color.r()), mix(color.g()), mix(color.b()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn equally_due_recurring_tasks_split_their_weight_evenly() {
        let weights = [1.0, 100.0, 100.0];
        assert_eq!(weighted_choice_index(&weights, 0.25), 1);
        assert_eq!(weighted_choice_index(&weights, 0.75), 2);
    }
}
