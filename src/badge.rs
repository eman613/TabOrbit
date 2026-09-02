use windows::core::{w, PCWSTR};
use windows::Win32::Graphics::GdiPlus::{
    FontStyleBold, GdipCreateFont, GdipCreateFontFamilyFromName, GdipCreateSolidFill,
    GdipCreateStringFormat, GdipDeleteBrush, GdipDeleteFont, GdipDeleteFontFamily,
    GdipDeleteStringFormat, GdipDrawString, GdipSetStringFormatAlign, GdipSetStringFormatLineAlign,
    GdipSetTextRenderingHint, GpBrush, GpFont, GpFontFamily, GpGraphics, GpSolidFill,
    GpStringFormat, RectF, StringAlignmentCenter, TextRenderingHintAntiAliasGridFit, UnitPixel,
};

const BADGE_HEIGHT_BASE: i32 = 20;
const BADGE_OFFSET_BASE: i32 = 2;
const BADGE_RADIUS_BASE: i32 = 10;
const BADGE_FONT_SIZE_BASE: i32 = 11;
const BADGE_SINGLE_WIDTH_BASE: i32 = 20;
const BADGE_EXTRA_WIDTH_PER_CHAR_BASE: i32 = 5;
pub const BADGE_BACKGROUND: u32 = 0xff617285;
const BADGE_FOREGROUND: u32 = 0xffabc0d6;

pub fn draw_badge_text(
    graphics: *mut GpGraphics,
    count: usize,
    bounds: (i32, i32, i32, i32),
    dpi_scale: f64,
) {
    if graphics.is_null() || !should_show(count) || dpi_scale <= 0.0 {
        return;
    }
    let (left, top, right, bottom) = bounds;
    if right <= left || bottom <= top {
        return;
    }

    let mut family: *mut GpFontFamily = std::ptr::null_mut();
    let mut font: *mut GpFont = std::ptr::null_mut();
    let mut format: *mut GpStringFormat = std::ptr::null_mut();
    let mut solid_fill: *mut GpSolidFill = std::ptr::null_mut();

    unsafe {
        let status =
            GdipCreateFontFamilyFromName(w!("Segoe UI"), std::ptr::null_mut(), &mut family);
        if status.0 != 0 || family.is_null() {
            return;
        }
        let font_size = scaled(BADGE_FONT_SIZE_BASE, dpi_scale) as f32;
        let status = GdipCreateFont(family, font_size, FontStyleBold.0, UnitPixel, &mut font);
        if status.0 != 0 || font.is_null() {
            GdipDeleteFontFamily(family);
            return;
        }
        let status = GdipCreateStringFormat(0, 0, &mut format);
        if status.0 != 0 || format.is_null() {
            GdipDeleteFont(font);
            GdipDeleteFontFamily(family);
            return;
        }
        GdipSetStringFormatAlign(format, StringAlignmentCenter);
        GdipSetStringFormatLineAlign(format, StringAlignmentCenter);

        let status = GdipCreateSolidFill(BADGE_FOREGROUND, &mut solid_fill);
        if status.0 != 0 || solid_fill.is_null() {
            GdipDeleteStringFormat(format);
            GdipDeleteFont(font);
            GdipDeleteFontFamily(family);
            return;
        }

        let label = label(count);
        let mut text = label
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>();
        let layout = RectF {
            X: left as f32,
            Y: top as f32,
            Width: (right - left) as f32,
            Height: (bottom - top) as f32,
        };
        GdipSetTextRenderingHint(graphics, TextRenderingHintAntiAliasGridFit);
        GdipDrawString(
            graphics,
            PCWSTR(text.as_mut_ptr()),
            label.encode_utf16().count() as i32,
            font,
            &layout,
            format,
            solid_fill as *const GpBrush,
        );

        GdipDeleteBrush(solid_fill as *mut GpBrush);
        GdipDeleteStringFormat(format);
        GdipDeleteFont(font);
        GdipDeleteFontFamily(family);
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
