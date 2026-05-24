//! Pango-bordered tooltip primitives shared by every vendor renderer.
//!
//! Extracted from the per-vendor implementations because every tooltip
//! (Anthropic, OpenAI, Z.AI, OpenRouter) draws the same kind of box: blue
//! corners + horizontals, dim separators, centered title, left-padded body
//! lines. The only thing that varies is the line content.
//!
//! Mirrors the visual style of `claudebar`'s `${B}╭${border_h}╮${E}` block
//! (claudebar:843-859).

use crate::pango::visible_width;
use crate::theme::Theme;

/// One row of the bordered tooltip box.
pub enum Line {
    /// Centered text. The renderer pads both sides equally.
    Center(String),
    /// Body text. Left-justified, right-padded to fill the box.
    Body(String),
    /// A horizontal separator drawn with `─` characters.
    Sep,
}

/// Render the bordered tooltip. Width is computed from the widest body/center
/// line so different vendors auto-size correctly.
pub fn render_bordered(lines: &[Line], theme: &Theme) -> String {
    let blue = &theme.blue;
    let dim = &theme.dim;

    let mut max_w: usize = 0;
    for line in lines {
        let s = match line {
            Line::Center(s) | Line::Body(s) => s.as_str(),
            Line::Sep => continue,
        };
        let w = visible_width(s);
        if w > max_w {
            max_w = w;
        }
    }
    let inner_w = max_w + 1;
    let border_h: String = "─".repeat(inner_w);
    let sep_inner: String = "─".repeat(inner_w.saturating_sub(2));
    let sep_line = format!(" <span foreground='{dim}'>{sep_inner}</span>");

    let mut out = String::with_capacity(256 * lines.len());
    out.push_str(&format!("<span foreground='{blue}'>╭{border_h}╮</span>\n"));
    for line in lines {
        let body = match line {
            Line::Body(s) => pad_right(s, inner_w),
            Line::Center(s) => pad_center(s, inner_w),
            Line::Sep => pad_right(&sep_line, inner_w),
        };
        out.push_str(&format!(
            "<span foreground='{blue}'>│</span>{body}<span foreground='{blue}'>│</span>\n"
        ));
    }
    out.push_str(&format!("<span foreground='{blue}'>╰{border_h}╯</span>"));
    out
}

/// Pad `s` on the right with spaces so its visible width reaches `inner_w`.
pub fn pad_right(s: &str, inner_w: usize) -> String {
    let v = visible_width(s);
    let need = inner_w.saturating_sub(v);
    format!("{s}{}", " ".repeat(need))
}

/// Pad `s` symmetrically; when the difference is odd, the extra space goes
/// on the right (claudebar `center_pad` precedent).
pub fn pad_center(s: &str, inner_w: usize) -> String {
    let v = visible_width(s);
    let total = inner_w.saturating_sub(v);
    let lp = total / 2;
    let rp = total - lp;
    format!("{}{s}{}", " ".repeat(lp), " ".repeat(rp))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        Theme::default()
    }

    #[test]
    fn renders_top_and_bottom_borders() {
        let lines = vec![Line::Center("Hi".into())];
        let out = render_bordered(&lines, &theme());
        assert!(out.contains("╭"));
        assert!(out.contains("╮"));
        assert!(out.contains("╰"));
        assert!(out.contains("╯"));
        assert!(out.contains("Hi"));
    }

    #[test]
    fn body_line_is_right_padded_to_inner_width() {
        // Box width = visible_width(widest) + 1 = "longest" (7) + 1 = 8.
        let lines = vec![
            Line::Center("a".into()),
            Line::Body("longest".into()),
        ];
        let out = render_bordered(&lines, &theme());
        // The body line should be padded so the right `│` lands at inner_w + 2.
        // We don't assert exact character offsets (Pango spans intervene), just
        // that the resulting markup is well-formed (open/close balanced).
        let opens = out.matches("<span").count();
        let closes = out.matches("</span>").count();
        assert_eq!(opens, closes);
    }

    #[test]
    fn pad_right_strips_pango_tags_before_measuring() {
        let s = "<span foreground='#fff'>abc</span>"; // visible width 3
        let p = pad_right(s, 6);
        // 3 padding spaces appended.
        assert!(p.ends_with("   "));
    }

    #[test]
    fn pad_center_distributes_extra_space_right_for_odd_diff() {
        let p = pad_center("X", 4); // visible 1, total padding 3 → lp=1, rp=2
        assert_eq!(p, " X  ");
    }

    #[test]
    fn separator_line_width_grows_with_content() {
        let lines = vec![
            Line::Center("a".into()),
            Line::Sep,
            Line::Body("longer body line".into()),
        ];
        let out = render_bordered(&lines, &theme());
        // The separator should reach the inner width of the box (just check
        // that it contains the unicode dash glyph repeated).
        assert!(out.contains("─"));
    }
}
