use windows::Win32::{
    Foundation::{COLORREF, RECT},
    Graphics::Gdi::{
        CreateFontW, CreateRoundRectRgn, DeleteObject, DrawTextW, FillRgn, SelectObject, SetBkMode,
        SetTextColor, ANTIALIASED_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DEFAULT_PITCH,
        DT_CENTER, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_SEMIBOLD,
        OUT_TT_ONLY_PRECIS, TRANSPARENT,
    },
    Graphics::Gdi::{HDC, HGDIOBJ},
};

const BADGE_HEIGHT_BASE: i32 = 20;
const BADGE_OFFSET_BASE: i32 = 2;
const BADGE_RADIUS_BASE: i32 = 10;
const BADGE_FONT_SIZE_BASE: i32 = 11;
const BADGE_SINGLE_WIDTH_BASE: i32 = 20;
const BADGE_EXTRA_WIDTH_PER_CHAR_BASE: i32 = 5;
const BADGE_BACKGROUND: u32 = 0x00617285;
const BADGE_FOREGROUND: u32 = 0x00abc0d6;

pub fn draw_badge_pixels(
    hdc: HDC,
    count: usize,
    icon_left: i32,
    icon_top: i32,
    icon_size: i32,
    dpi_scale: f64,
) {
    let Some((left, top, right, bottom, radius)) =
        badge_bounds_pixels(count, icon_left, icon_top, icon_size, dpi_scale)
    else {
        return;
    };
    draw_badge_with_bounds(hdc, count, (left, top, right, bottom, radius), dpi_scale);
}

fn draw_badge_with_bounds(
    hdc: HDC,
    count: usize,
    (left, top, right, bottom, radius): (i32, i32, i32, i32, i32),
    badge_scale: f64,
) {
    let label = label(count);

    unsafe {
        let region = CreateRoundRectRgn(left, top, right, bottom, radius, radius);
        if region.0.is_null() {
            return;
        }
        let brush = windows::Win32::Graphics::Gdi::CreateSolidBrush(COLORREF(BADGE_BACKGROUND));
        if !brush.0.is_null() {
            let _ = FillRgn(hdc, region, brush);
            let _ = DeleteObject(brush.into());
        }
        let _ = DeleteObject(region.into());

        let font = CreateFontW(
            -scaled(BADGE_FONT_SIZE_BASE, badge_scale),
            0,
            0,
            0,
            FW_SEMIBOLD.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_TT_ONLY_PRECIS,
            CLIP_DEFAULT_PRECIS,
            ANTIALIASED_QUALITY,
            DEFAULT_PITCH.0 as u32 | FF_DONTCARE.0 as u32,
            windows::core::w!("Segoe UI"),
        );
        if font.0.is_null() {
            return;
        }

        let old_font = SelectObject(hdc, font.into());
        let old_mode = SetBkMode(hdc, TRANSPARENT);
        let old_color = SetTextColor(hdc, COLORREF(BADGE_FOREGROUND));
        let mut rect = RECT {
            left,
            top,
            right,
            bottom,
        };
        let mut text = label.encode_utf16().collect::<Vec<u16>>();
        let _ = DrawTextW(
            hdc,
            &mut text,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
        );
        let _ = SetTextColor(hdc, old_color);
        let _ = SetBkMode(
            hdc,
            windows::Win32::Graphics::Gdi::BACKGROUND_MODE(old_mode as u32),
        );
        if !old_font.0.is_null() {
            let _ = SelectObject(hdc, old_font);
        }
        let _ = DeleteObject(HGDIOBJ(font.0));
    }
}

pub fn badge_bounds_pixels(
    count: usize,
    icon_left: i32,
    icon_top: i32,
    icon_size: i32,
    dpi_scale: f64,
) -> Option<(i32, i32, i32, i32, i32)> {
    if !should_show(count) || dpi_scale <= 0.0 || icon_size <= 0 {
        return None;
    }

    let label = label(count);
    let badge_height = scaled(BADGE_HEIGHT_BASE, dpi_scale);
    let badge_width = badge_width(label.encode_utf16().count(), dpi_scale);
    let offset = scaled(BADGE_OFFSET_BASE, dpi_scale);
    let right = icon_left.checked_add(icon_size)?.checked_sub(offset)?;
    let left = right.checked_sub(badge_width)?;
    let top = icon_top.checked_add(offset)?;
    let bottom = top.checked_add(badge_height)?;
    let radius = scaled(BADGE_RADIUS_BASE, dpi_scale);
    Some((left, top, right, bottom, radius))
}

pub const fn should_show(count: usize) -> bool {
    count > 1
}

pub fn label(count: usize) -> String {
    match count {
        0..=1 => String::new(),
        2..=99 => count.to_string(),
        _ => "99+".to_string(),
    }
}

const fn scaled(value: i32, scale: f64) -> i32 {
    (value as f64 * scale).round() as i32
}

const fn badge_width(char_count: usize, scale: f64) -> i32 {
    scaled(
        BADGE_SINGLE_WIDTH_BASE
            + BADGE_EXTRA_WIDTH_PER_CHAR_BASE * char_count.saturating_sub(1) as i32,
        scale,
    )
}

#[cfg(test)]
mod tests {
    use super::{badge_bounds_pixels, label, should_show};

    #[test]
    fn badge_visibility_uses_filtered_window_count() {
        assert!(!should_show(0));
        assert!(!should_show(1));
        assert!(should_show(2));
    }

    #[test]
    fn badge_label_caps_at_99() {
        assert_eq!(label(2), "2");
        assert_eq!(label(9), "9");
        assert_eq!(label(10), "10");
        assert_eq!(label(99), "99");
        assert_eq!(label(100), "99+");
    }

    #[test]
    fn badge_bounds_keep_round_corner_dimensions() {
        let single_digit = badge_bounds_pixels(2, 0, 0, 64, 1.0).unwrap();
        assert_eq!(single_digit.2 - single_digit.0, 20);
        assert_eq!(single_digit.3 - single_digit.1, 20);
        assert_eq!(single_digit.4, 10);

        let capped_count = badge_bounds_pixels(100, 0, 0, 64, 1.0).unwrap();
        assert_eq!(capped_count.2 - capped_count.0, 30);
        assert_eq!(capped_count.3 - capped_count.1, 20);
        assert_eq!(capped_count.4, 10);
    }
}
