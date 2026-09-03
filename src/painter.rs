use crate::app::SwitchAppsState;
use crate::badge::{badge_bounds_pixels, draw_badge_text, BADGE_BACKGROUND};
use crate::utils::{check_error, get_moinitor_rect, is_light_theme, is_win11};

use anyhow::{Context, Result};
use std::{ffi::c_void, mem};
use windows::core::{w, PCWSTR};
use windows::Win32::{
    Foundation::{COLORREF, HWND, POINT, SIZE},
    Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND},
    Graphics::{
        Gdi::{
            CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
            SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
            BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC,
        },
        GdiPlus::{
            FillModeAlternate, FontStyleRegular, GdipAddPathArc, GdipClosePathFigure,
            GdipCreateBitmapFromHICON, GdipCreateFont, GdipCreateFontFamilyFromName,
            GdipCreateFromHDC, GdipCreatePath, GdipCreateSolidFill, GdipCreateStringFormat,
            GdipDeleteBrush, GdipDeleteFont, GdipDeleteFontFamily, GdipDeleteGraphics,
            GdipDeletePath, GdipDeleteStringFormat, GdipDisposeImage, GdipDrawImageRect,
            GdipDrawString, GdipFillPath, GdipGraphicsClear, GdipSetInterpolationMode,
            GdipSetSmoothingMode, GdipSetStringFormatAlign, GdipSetStringFormatFlags,
            GdipSetStringFormatLineAlign, GdipSetStringFormatTrimming, GdipSetTextRenderingHint,
            GdiplusShutdown, GdiplusStartup, GdiplusStartupInput, GpBitmap, GpBrush, GpGraphics,
            GpImage, GpPath, GpSolidFill, InterpolationModeHighQualityBicubic, RectF,
            SmoothingModeAntiAlias, StringAlignmentCenter, StringFormatFlagsNoWrap,
            StringTrimmingEllipsisCharacter, TextRenderingHintAntiAliasGridFit, UnitPixel,
        },
    },
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::SetFocus,
        WindowsAndMessaging::{
            GetCursorPos, ShowWindow, UpdateLayeredWindow, SW_HIDE, SW_SHOW, ULW_ALPHA,
        },
    },
};

pub const BG_DARK_COLOR: u32 = 0x64191919;
pub const FG_DARK_COLOR: u32 = 0xb0505050;
pub const BG_LIGHT_COLOR: u32 = 0x64f4f4f4;
pub const FG_LIGHT_COLOR: u32 = 0xb0d0d0d0;
pub const ICON_SIZE_BASE: i32 = 64;
pub const WINDOW_BORDER_SIZE_BASE: i32 = 10;
pub const ICON_BORDER_SIZE_BASE: i32 = 4;
const LABEL_GAP_BASE: i32 = 4;
const LABEL_HEIGHT_BASE: i32 = 24;
const LABEL_FONT_SIZE_BASE: i32 = 13;
const LABEL_MAX_WIDTH_BASE: i32 = 220;
const PANEL_CORNER_RADIUS_BASE: i32 = 16;
const ITEM_CORNER_RADIUS_BASE: i32 = 8;
const SELECTION_INSET_BASE: i32 = 1;
const TRANSPARENT_COLOR: u32 = 0x00000000;

// GDI Antialiasing Painter
pub struct GdiAAPainter {
    token: usize,
    hwnd: HWND,
    hdc_screen: HDC,
    show: bool,
}

impl GdiAAPainter {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let startup_input = GdiplusStartupInput {
            GdiplusVersion: 1,
            ..Default::default()
        };
        let mut token: usize = 0;
        check_error(|| unsafe { GdiplusStartup(&mut token, &startup_input, std::ptr::null_mut()) })
            .context("Failed to initialize GDI+")?;

        let hdc_screen = unsafe { GetDC(Some(hwnd)) };
        configure_window_visuals(hwnd);

        Ok(Self {
            token,
            hwnd,
            hdc_screen,
            show: false,
        })
    }

    pub fn paint(&mut self, state: &SwitchAppsState) {
        let dpi_scale = get_dpi_scale(self.hwnd);
        let icon_size_max = (ICON_SIZE_BASE as f64 * dpi_scale) as i32;
        let border_size = (WINDOW_BORDER_SIZE_BASE as f64 * dpi_scale) as i32;
        let icon_border = (ICON_BORDER_SIZE_BASE as f64 * dpi_scale) as i32;
        let label_gap = (LABEL_GAP_BASE as f64 * dpi_scale) as i32;
        let label_height = (LABEL_HEIGHT_BASE as f64 * dpi_scale) as i32;

        let Coordinate {
            x,
            y,
            width,
            height,
            icon_size,
            item_size,
        } = Coordinate::new(
            state.apps.len() as i32,
            icon_size_max,
            border_size,
            icon_border,
            label_gap,
            label_height,
        );

        let panel_corner_radius = (PANEL_CORNER_RADIUS_BASE as f64 * dpi_scale) as i32;
        let item_corner_radius = (ITEM_CORNER_RADIUS_BASE as f64 * dpi_scale) as i32;

        let hwnd = self.hwnd;
        let hdc_screen = self.hdc_screen;

        let (panel_color, selected_color, label_color) = theme_color(is_light_theme());
        let selection_inset = ((SELECTION_INSET_BASE as f64 * dpi_scale).round() as i32).max(1);

        unsafe {
            let hdc_mem = CreateCompatibleDC(Some(hdc_screen));
            let Some(panel_surface) = create_argb_bitmap(hdc_screen, width, height) else {
                error!("failed to create ARGB panel bitmap");
                let _ = DeleteDC(hdc_mem);
                return;
            };
            let bitmap_mem = panel_surface.bitmap;
            SelectObject(hdc_mem, bitmap_mem.into());

            let mut graphics = GpGraphics::default();
            let mut graphics_ptr: *mut GpGraphics = &mut graphics;
            GdipCreateFromHDC(hdc_mem, &mut graphics_ptr as _);
            GdipSetSmoothingMode(graphics_ptr, SmoothingModeAntiAlias);
            GdipSetInterpolationMode(graphics_ptr, InterpolationModeHighQualityBicubic);
            GdipGraphicsClear(graphics_ptr, TRANSPARENT_COLOR);
            let panel_brush = create_solid_brush(panel_color);
            if !panel_brush.is_null() {
                draw_round_rect(
                    graphics_ptr,
                    panel_brush,
                    0.0,
                    0.0,
                    width as f32,
                    height as f32,
                    panel_corner_radius.min(width.min(height) / 2) as f32,
                );
            }

            draw_icons(
                graphics_ptr,
                state,
                border_size,
                icon_border,
                icon_size,
                item_size,
                item_corner_radius.min(item_size / 2),
                selected_color,
                selection_inset,
            );
            draw_badges(
                graphics_ptr,
                state,
                border_size,
                icon_border,
                icon_size,
                item_size,
                dpi_scale,
            );
            draw_label(
                graphics_ptr,
                state,
                width,
                border_size,
                item_size,
                label_gap,
                label_height,
                dpi_scale,
                label_color,
            );
            premultiply_alpha(panel_surface.bits, width, height);

            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as _,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as _,
                ..Default::default()
            };
            let _ = UpdateLayeredWindow(
                hwnd,
                Some(hdc_screen),
                Some(&POINT { x, y }),
                Some(&SIZE {
                    cx: width,
                    cy: height,
                }),
                Some(hdc_mem),
                Some(&POINT::default()),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );

            if !panel_brush.is_null() {
                GdipDeleteBrush(panel_brush);
            }
            if !graphics_ptr.is_null() {
                GdipDeleteGraphics(graphics_ptr);
            }

            let _ = DeleteObject(bitmap_mem.into());
            let _ = DeleteDC(hdc_mem);
        }

        if self.show {
            return;
        }
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOW);
            let _ = SetFocus(Some(self.hwnd));
        }
        self.show = true;
    }

    pub fn unpaint(&mut self, _state: SwitchAppsState) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
        self.show = false;
    }

    pub fn find_clicked_app_index(&self, state: &SwitchAppsState) -> Option<usize> {
        let cursor_pos = unsafe {
            let mut pos = POINT::default();
            let _ = GetCursorPos(&mut pos);
            pos
        };

        let dpi_scale = get_dpi_scale(self.hwnd);
        let icon_size_max = (ICON_SIZE_BASE as f64 * dpi_scale) as i32;
        let border_size = (WINDOW_BORDER_SIZE_BASE as f64 * dpi_scale) as i32;
        let icon_border = (ICON_BORDER_SIZE_BASE as f64 * dpi_scale) as i32;
        let label_gap = (LABEL_GAP_BASE as f64 * dpi_scale) as i32;
        let label_height = (LABEL_HEIGHT_BASE as f64 * dpi_scale) as i32;

        let Coordinate {
            x, y, item_size, ..
        } = Coordinate::new(
            state.apps.len() as i32,
            icon_size_max,
            border_size,
            icon_border,
            label_gap,
            label_height,
        );

        let xpos = cursor_pos.x - x;
        let ypos = cursor_pos.y - y;

        let cy = border_size;
        for (i, _) in state.apps.iter().enumerate() {
            let cx = border_size + item_size * (i as i32);
            if xpos >= cx && xpos < cx + item_size && ypos >= cy && ypos < cy + item_size {
                return Some(i);
            }
        }
        None
    }
}

impl Drop for GdiAAPainter {
    fn drop(&mut self) {
        unsafe {
            ReleaseDC(Some(self.hwnd), self.hdc_screen);
            GdiplusShutdown(self.token);
        }
    }
}

const fn theme_color(light_theme: bool) -> (u32, u32, u32) {
    match light_theme {
        true => (BG_LIGHT_COLOR, FG_LIGHT_COLOR, 0xff303030),
        false => (BG_DARK_COLOR, FG_DARK_COLOR, 0xfff0f0f0),
    }
}

unsafe fn draw_round_rect(
    graphic_ptr: *mut GpGraphics,
    brush_ptr: *mut GpBrush,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
    corner_radius: f32,
) {
    unsafe {
        let diameter = corner_radius * 2.0;
        let mut path = GpPath::default();
        let mut path_ptr: *mut GpPath = &mut path;
        GdipCreatePath(FillModeAlternate, &mut path_ptr as _);
        GdipAddPathArc(path_ptr, left, top, diameter, diameter, 180.0, 90.0);
        GdipAddPathArc(
            path_ptr,
            right - diameter,
            top,
            diameter,
            diameter,
            270.0,
            90.0,
        );
        GdipAddPathArc(
            path_ptr,
            right - diameter,
            bottom - diameter,
            diameter,
            diameter,
            0.0,
            90.0,
        );
        GdipAddPathArc(
            path_ptr,
            left,
            bottom - diameter,
            diameter,
            diameter,
            90.0,
            90.0,
        );
        GdipClosePathFigure(path_ptr);
        GdipFillPath(graphic_ptr, brush_ptr, path_ptr);
        GdipDeletePath(path_ptr);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_icons(
    graphics: *mut GpGraphics,
    state: &SwitchAppsState,
    border_size: i32,
    icon_border: i32,
    icon_size: i32,
    item_size: i32,
    corner_radius: i32,
    selected_color: u32,
    selection_inset: i32,
) {
    if graphics.is_null() || icon_size <= 0 || item_size <= 0 {
        return;
    }

    let selected_brush = unsafe { create_solid_brush(selected_color) };
    for (i, entry) in state.apps.iter().enumerate() {
        let item_left = border_size + item_size * i as i32;
        if i == state.index && !selected_brush.is_null() {
            let inset = selection_inset.min(item_size / 2);
            let left = item_left + inset;
            let top = border_size + inset;
            let right = item_left + item_size - inset;
            let bottom = border_size + item_size - inset;
            let radius = corner_radius
                .saturating_sub(inset)
                .min((right - left).min(bottom - top) / 2);
            unsafe {
                draw_round_rect(
                    graphics,
                    selected_brush,
                    left as f32,
                    top as f32,
                    right as f32,
                    bottom as f32,
                    radius as f32,
                );
            }
        }

        let mut bitmap_ptr: *mut GpBitmap = std::ptr::null_mut();
        let status = unsafe { GdipCreateBitmapFromHICON(entry.icon, &mut bitmap_ptr) };
        if status != windows::Win32::Graphics::GdiPlus::Status(0) || bitmap_ptr.is_null() {
            debug!("failed to create GDI+ icon bitmap: index={i}, status={status:?}");
            continue;
        }

        unsafe {
            let image_ptr = bitmap_ptr as *mut GpImage;
            let draw_status = GdipDrawImageRect(
                graphics,
                image_ptr,
                (item_left + icon_border) as f32,
                (border_size + icon_border) as f32,
                icon_size as f32,
                icon_size as f32,
            );
            if draw_status != windows::Win32::Graphics::GdiPlus::Status(0) {
                debug!("failed to draw GDI+ icon: index={i}, status={draw_status:?}");
            }
            GdipDisposeImage(image_ptr);
        }
    }

    if !selected_brush.is_null() {
        unsafe {
            GdipDeleteBrush(selected_brush);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_badges(
    graphics: *mut GpGraphics,
    state: &SwitchAppsState,
    border_size: i32,
    icon_border: i32,
    icon_size: i32,
    item_size: i32,
    dpi_scale: f64,
) {
    if graphics.is_null() {
        return;
    }
    let badge_brush = unsafe { create_solid_brush(BADGE_BACKGROUND) };
    if badge_brush.is_null() {
        return;
    }
    let icon_top = border_size + icon_border;
    for (i, entry) in state.apps.iter().enumerate() {
        let icon_left = border_size + icon_border + item_size * i as i32;
        if let Some((left, top, right, bottom, radius)) = badge_bounds_pixels(
            entry.window_count,
            icon_left,
            icon_top,
            icon_size,
            dpi_scale,
        ) {
            unsafe {
                draw_round_rect(
                    graphics,
                    badge_brush,
                    left as f32,
                    top as f32,
                    right as f32,
                    bottom as f32,
                    radius as f32,
                );
            }
            draw_badge_text(
                graphics,
                entry.window_count,
                (left, top, right, bottom),
                dpi_scale,
            );
        }
    }
    unsafe {
        GdipDeleteBrush(badge_brush);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_label(
    graphics: *mut GpGraphics,
    state: &SwitchAppsState,
    panel_width: i32,
    border_size: i32,
    item_size: i32,
    label_gap: i32,
    label_height: i32,
    dpi_scale: f64,
    label_color: u32,
) {
    if graphics.is_null()
        || state.index >= state.apps.len()
        || item_size <= 0
        || label_height <= 0
        || dpi_scale <= 0.0
    {
        return;
    }

    let text = state.apps[state.index].display_name.trim();
    if text.is_empty() {
        return;
    }

    let content_left = border_size;
    let content_right = panel_width.saturating_sub(border_size);
    let available_width = content_right.saturating_sub(content_left);
    let label_width = scaled(LABEL_MAX_WIDTH_BASE, dpi_scale)
        .min(available_width)
        .max(1);
    let center_x = border_size + item_size * state.index as i32 + item_size / 2;
    let min_left = content_left;
    let max_left = content_right.saturating_sub(label_width);
    let left = (center_x - label_width / 2).clamp(min_left, max_left);
    let right = left.saturating_add(label_width);
    let width = right.saturating_sub(left);
    if width <= 0 {
        return;
    }

    let top = border_size + item_size + label_gap;
    let layout = RectF {
        X: left as f32,
        Y: top as f32,
        Width: width as f32,
        Height: label_height as f32,
    };
    let text_utf16: Vec<u16> = text.encode_utf16().collect();

    unsafe {
        let mut family = std::ptr::null_mut();
        let mut status = GdipCreateFontFamilyFromName(
            w!("Microsoft YaHei UI"),
            std::ptr::null_mut(),
            &mut family,
        );
        if status.0 != 0 || family.is_null() {
            family = std::ptr::null_mut();
            status =
                GdipCreateFontFamilyFromName(w!("Segoe UI"), std::ptr::null_mut(), &mut family);
        }
        if status.0 != 0 || family.is_null() {
            debug!("failed to create application label font family");
            return;
        }

        let mut font = std::ptr::null_mut();
        status = GdipCreateFont(
            family,
            scaled(LABEL_FONT_SIZE_BASE, dpi_scale) as f32,
            FontStyleRegular.0,
            UnitPixel,
            &mut font,
        );
        if status.0 != 0 || font.is_null() {
            GdipDeleteFontFamily(family);
            return;
        }

        let mut format = std::ptr::null_mut();
        status = GdipCreateStringFormat(0, 0, &mut format);
        if status.0 != 0 || format.is_null() {
            GdipDeleteFont(font);
            GdipDeleteFontFamily(family);
            return;
        }
        GdipSetStringFormatAlign(format, StringAlignmentCenter);
        GdipSetStringFormatLineAlign(format, StringAlignmentCenter);
        GdipSetStringFormatFlags(format, StringFormatFlagsNoWrap.0);
        GdipSetStringFormatTrimming(format, StringTrimmingEllipsisCharacter);

        let mut brush = std::ptr::null_mut();
        status = GdipCreateSolidFill(label_color, &mut brush);
        if status.0 != 0 || brush.is_null() {
            GdipDeleteStringFormat(format);
            GdipDeleteFont(font);
            GdipDeleteFontFamily(family);
            return;
        }

        GdipSetTextRenderingHint(graphics, TextRenderingHintAntiAliasGridFit);
        let _ = GdipDrawString(
            graphics,
            PCWSTR(text_utf16.as_ptr()),
            text_utf16.len() as i32,
            font,
            &layout,
            format,
            brush as *const GpBrush,
        );

        GdipDeleteBrush(brush as *mut GpBrush);
        GdipDeleteStringFormat(format);
        GdipDeleteFont(font);
        GdipDeleteFontFamily(family);
    }
}

struct ArgbBitmap {
    bitmap: HBITMAP,
    bits: *mut c_void,
}

fn premultiply_alpha(bits: *mut c_void, bitmap_width: i32, bitmap_height: i32) {
    if bits.is_null() || bitmap_width <= 0 || bitmap_height <= 0 {
        return;
    }
    unsafe {
        let pixels = std::slice::from_raw_parts_mut(
            bits as *mut u8,
            bitmap_width as usize * bitmap_height as usize * 4,
        );
        let (pixel_chunks, _) = pixels.as_chunks_mut::<4>();
        for pixel in pixel_chunks {
            let alpha = u16::from(pixel[3]);
            pixel[0] = ((u16::from(pixel[0]) * alpha + 127) / 255) as u8;
            pixel[1] = ((u16::from(pixel[1]) * alpha + 127) / 255) as u8;
            pixel[2] = ((u16::from(pixel[2]) * alpha + 127) / 255) as u8;
        }
    }
}

fn create_argb_bitmap(hdc: HDC, width: i32, height: i32) -> Option<ArgbBitmap> {
    if width <= 0 || height <= 0 {
        return None;
    }
    let byte_len = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(4)?;

    let bitmap_info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    let bitmap = unsafe {
        CreateDIBSection(Some(hdc), &bitmap_info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?
    };
    if bits.is_null() {
        unsafe {
            let _ = DeleteObject(bitmap.into());
        }
        return None;
    }
    unsafe {
        std::ptr::write_bytes(bits, 0, byte_len);
    }
    Some(ArgbBitmap { bitmap, bits })
}

unsafe fn create_solid_brush(color: u32) -> *mut GpBrush {
    let mut solid_fill: *mut GpSolidFill = std::ptr::null_mut();
    if GdipCreateSolidFill(color, &mut solid_fill) != windows::Win32::Graphics::GdiPlus::Status(0) {
        return std::ptr::null_mut();
    }
    solid_fill as *mut GpBrush
}

fn configure_window_visuals(hwnd: HWND) {
    if is_win11() {
        let preference = DWMWCP_ROUND;
        let result = unsafe {
            DwmSetWindowAttribute(
                hwnd,
                DWMWA_WINDOW_CORNER_PREFERENCE,
                &preference as *const _ as *const c_void,
                mem::size_of_val(&preference) as u32,
            )
        };
        if let Err(err) = result {
            debug!("DWM rounded corner unavailable: {err}");
        }
    }

    debug!("window blur disabled for transparent layered frame; using ARGB surface");
}

fn get_dpi_scale(hwnd: HWND) -> f64 {
    unsafe {
        let dpi = GetDpiForWindow(hwnd);
        if dpi == 0 {
            1.0
        } else {
            dpi as f64 / 96.0
        }
    }
}

const fn scaled(value: i32, scale: f64) -> i32 {
    (value as f64 * scale).round() as i32
}

struct Coordinate {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    icon_size: i32,
    item_size: i32,
}

impl Coordinate {
    fn new(
        num_apps: i32,
        icon_size_max: i32,
        border_size: i32,
        icon_border: i32,
        label_gap: i32,
        label_height: i32,
    ) -> Self {
        let monitor_rect = get_moinitor_rect();
        let monitor_width = monitor_rect.right - monitor_rect.left;
        let monitor_height = monitor_rect.bottom - monitor_rect.top;

        let icon_size =
            ((monitor_width - 2 * border_size) / num_apps - icon_border * 2).min(icon_size_max);

        let item_size = icon_size + icon_border * 2;
        let width = item_size * num_apps + border_size * 2;
        let height = item_size + label_gap + label_height + border_size * 2;
        let x = monitor_rect.left + (monitor_width - width) / 2;
        let y = monitor_rect.top + (monitor_height - height) / 2;

        Self {
            x,
            y,
            width,
            height,
            icon_size,
            item_size,
        }
    }
}
