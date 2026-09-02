use crate::app::SwitchAppsState;
use crate::badge::{badge_bounds_pixels, draw_badge_text, BADGE_BACKGROUND};
use crate::utils::{check_error, get_moinitor_rect, is_light_theme, is_win11};

use anyhow::{Context, Result};
use std::{ffi::c_void, mem};
use windows::core::{s, w, BOOL};
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
            FillModeAlternate, GdipAddPathArc, GdipClosePathFigure, GdipCreateBitmapFromScan0,
            GdipCreateFromHDC, GdipCreatePath, GdipCreateSolidFill, GdipDeleteBrush,
            GdipDeleteGraphics, GdipDeletePath, GdipDisposeImage, GdipDrawImageRect, GdipFillPath,
            GdipGraphicsClear, GdipSetInterpolationMode, GdipSetSmoothingMode, GdiplusShutdown,
            GdiplusStartup, GdiplusStartupInput, GpBrush, GpGraphics, GpPath, GpSolidFill,
            InterpolationModeHighQualityBicubic, SmoothingModeAntiAlias,
        },
    },
    System::LibraryLoader::{GetModuleHandleW, GetProcAddress},
    UI::{
        HiDpi::GetDpiForWindow,
        Input::KeyboardAndMouse::SetFocus,
        WindowsAndMessaging::{
            DrawIconEx, GetCursorPos, ShowWindow, UpdateLayeredWindow, DI_NORMAL, SW_HIDE, SW_SHOW,
            ULW_ALPHA,
        },
    },
};

pub const BG_DARK_COLOR: u32 = 0x64191919;
pub const FG_DARK_COLOR: u32 = 0xc8505050;
pub const BG_LIGHT_COLOR: u32 = 0x64f4f4f4;
pub const FG_LIGHT_COLOR: u32 = 0xc8d0d0d0;
pub const ICON_SIZE_BASE: i32 = 64;
pub const WINDOW_BORDER_SIZE_BASE: i32 = 10;
pub const ICON_BORDER_SIZE_BASE: i32 = 4;
pub const SCALE_FACTOR: i32 = 6;
const PANEL_CORNER_RADIUS_BASE: i32 = 16;
const ITEM_CORNER_RADIUS_BASE: i32 = 8;
const PIXEL_FORMAT_32BPP_ARGB: i32 = 0x26200a;

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
        );

        let panel_corner_radius = (PANEL_CORNER_RADIUS_BASE as f64 * dpi_scale) as i32;
        let item_corner_radius = (ITEM_CORNER_RADIUS_BASE as f64 * dpi_scale) as i32;

        let hwnd = self.hwnd;
        let hdc_screen = self.hdc_screen;

        let (panel_color, selected_color) = theme_color(is_light_theme());

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
            GdipGraphicsClear(graphics_ptr, panel_color);
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

            let icons_width = item_size * state.apps.len() as i32;
            let icons_height = item_size;
            let Some(bitmap_icons) = draw_icons(
                state,
                hdc_screen,
                icon_size,
                icon_border,
                icons_width,
                icons_height,
                item_corner_radius.min(item_size / 2),
                selected_color,
                panel_color,
            ) else {
                error!("failed to create ARGB icon bitmap");
                if !panel_brush.is_null() {
                    GdipDeleteBrush(panel_brush);
                }
                GdipDeleteGraphics(graphics_ptr);
                let _ = DeleteObject(bitmap_mem.into());
                let _ = DeleteDC(hdc_mem);
                return;
            };

            let mut bitmap_ptr = std::ptr::null_mut();
            let status = GdipCreateBitmapFromScan0(
                icons_width * SCALE_FACTOR,
                icons_height * SCALE_FACTOR,
                icons_width * SCALE_FACTOR * 4,
                PIXEL_FORMAT_32BPP_ARGB,
                Some(bitmap_icons.bits as *const u8),
                &mut bitmap_ptr,
            );
            if status != windows::Win32::Graphics::GdiPlus::Status(0) {
                error!("failed to create GDI+ icon image: status={status:?}");
            } else {
                let image_ptr = bitmap_ptr as *mut windows::Win32::Graphics::GdiPlus::GpImage;
                GdipDrawImageRect(
                    graphics_ptr,
                    image_ptr,
                    border_size as f32,
                    border_size as f32,
                    icons_width as f32,
                    icons_height as f32,
                );
                GdipDisposeImage(image_ptr);
                draw_badges(
                    graphics_ptr,
                    state,
                    border_size,
                    icon_border,
                    icon_size,
                    item_size,
                    dpi_scale,
                );
            }
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

            let _ = DeleteObject(bitmap_icons.bitmap.into());
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

        let Coordinate {
            x, y, item_size, ..
        } = Coordinate::new(
            state.apps.len() as i32,
            icon_size_max,
            border_size,
            icon_border,
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

const fn theme_color(light_theme: bool) -> (u32, u32) {
    match light_theme {
        true => (FG_LIGHT_COLOR, BG_LIGHT_COLOR),
        false => (FG_DARK_COLOR, BG_DARK_COLOR),
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
        let mut path = GpPath::default();
        let mut path_ptr: *mut GpPath = &mut path;
        GdipCreatePath(FillModeAlternate, &mut path_ptr as _);
        GdipAddPathArc(
            path_ptr,
            left,
            top,
            corner_radius,
            corner_radius,
            180.0,
            90.0,
        );
        GdipAddPathArc(
            path_ptr,
            right - corner_radius,
            top,
            corner_radius,
            corner_radius,
            270.0,
            90.0,
        );
        GdipAddPathArc(
            path_ptr,
            right - corner_radius,
            bottom - corner_radius,
            corner_radius,
            corner_radius,
            0.0,
            90.0,
        );
        GdipAddPathArc(
            path_ptr,
            left,
            bottom - corner_radius,
            corner_radius,
            corner_radius,
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
    state: &SwitchAppsState,
    hdc_screen: HDC,
    icon_size: i32,
    icon_border: i32,
    width: i32,
    height: i32,
    corner_radius: i32,
    selected_color: u32,
    transparent_color: u32,
) -> Option<ArgbBitmap> {
    let scaled_width = width * SCALE_FACTOR;
    let scaled_height = height * SCALE_FACTOR;
    let scaled_corner_radius = corner_radius * SCALE_FACTOR;
    let scaled_border_size = icon_border * SCALE_FACTOR;
    let scaled_icon_inner_size = icon_size * SCALE_FACTOR;
    let scaled_icon_outer_size = scaled_icon_inner_size + scaled_border_size * 2;
    let transparent_rgb = transparent_color & 0x00ff_ffff;
    debug!("icon surface transparent RGB: {transparent_rgb:#08x}");

    unsafe {
        let hdc_scaled = CreateCompatibleDC(Some(hdc_screen));
        let bitmap_scaled = create_argb_bitmap(hdc_screen, scaled_width, scaled_height)?;
        let bitmap_scaled_handle = bitmap_scaled.bitmap;
        SelectObject(hdc_scaled, bitmap_scaled.bitmap.into());

        let mut graphics_scaled: *mut GpGraphics = std::ptr::null_mut();
        GdipCreateFromHDC(hdc_scaled, &mut graphics_scaled);
        if !graphics_scaled.is_null() {
            GdipSetSmoothingMode(graphics_scaled, SmoothingModeAntiAlias);
            GdipSetInterpolationMode(graphics_scaled, InterpolationModeHighQualityBicubic);
            GdipGraphicsClear(graphics_scaled, 0);
        }
        clear_transparent_surface(
            bitmap_scaled.bits,
            scaled_width,
            scaled_height,
            transparent_rgb,
        );
        let selected_brush = create_solid_brush(selected_color);

        for (i, entry) in state.apps.iter().enumerate() {
            // draw the box for selected icon
            if i == state.index && !selected_brush.is_null() && !graphics_scaled.is_null() {
                let left = scaled_icon_outer_size * (i as i32);
                let top = 0;
                let right = left + scaled_icon_outer_size;
                let bottom = top + scaled_icon_outer_size;
                draw_round_rect(
                    graphics_scaled,
                    selected_brush,
                    left as f32,
                    top as f32,
                    right as f32,
                    bottom as f32,
                    scaled_corner_radius as f32,
                );
            }

            let cx = scaled_border_size + scaled_icon_outer_size * (i as i32);
            let _ = DrawIconEx(
                hdc_scaled,
                cx,
                scaled_border_size,
                entry.icon,
                scaled_icon_inner_size,
                scaled_icon_inner_size,
                0,
                None,
                DI_NORMAL,
            );
        }

        if !graphics_scaled.is_null() {
            GdipDeleteGraphics(graphics_scaled);
        }
        if !selected_brush.is_null() {
            GdipDeleteBrush(selected_brush);
        }
        let _ = DeleteDC(hdc_scaled);

        Some(ArgbBitmap {
            bitmap: bitmap_scaled_handle,
            bits: bitmap_scaled.bits,
        })
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

struct ArgbBitmap {
    bitmap: HBITMAP,
    bits: *mut c_void,
}

fn clear_transparent_surface(bits: *mut c_void, bitmap_width: i32, bitmap_height: i32, color: u32) {
    if bits.is_null() || bitmap_width <= 0 || bitmap_height <= 0 {
        return;
    }
    let blue = (color & 0xff) as u8;
    let green = ((color >> 8) & 0xff) as u8;
    let red = ((color >> 16) & 0xff) as u8;
    unsafe {
        let pixels = std::slice::from_raw_parts_mut(
            bits as *mut u8,
            bitmap_width as usize * bitmap_height as usize * 4,
        );
        let (pixel_chunks, _) = pixels.as_chunks_mut::<4>();
        for pixel in pixel_chunks {
            pixel[0] = blue;
            pixel[1] = green;
            pixel[2] = red;
            pixel[3] = 0;
        }
    }
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

#[repr(C)]
struct AccentPolicy {
    accent_state: u32,
    accent_flags: u32,
    gradient_color: u32,
    animation_id: u32,
}

#[repr(C)]
struct WindowCompositionAttributeData {
    attribute: u32,
    data: *mut c_void,
    data_size: usize,
}

type SetWindowCompositionAttributeFn =
    unsafe extern "system" fn(HWND, *mut WindowCompositionAttributeData) -> BOOL;

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

    if enable_blur(hwnd) {
        debug!("window blur enabled");
    } else {
        debug!("window blur unavailable; using translucent surface");
    }
}

fn enable_blur(hwnd: HWND) -> bool {
    let Ok(user32) = (unsafe { GetModuleHandleW(w!("user32.dll")) }) else {
        return false;
    };
    let Some(procedure) = (unsafe { GetProcAddress(user32, s!("SetWindowCompositionAttribute")) })
    else {
        return false;
    };
    let set_window_composition_attribute: SetWindowCompositionAttributeFn =
        unsafe { mem::transmute(procedure) };
    let mut policy = AccentPolicy {
        accent_state: 3,
        accent_flags: 0,
        gradient_color: 0,
        animation_id: 0,
    };
    let mut data = WindowCompositionAttributeData {
        attribute: 19,
        data: &mut policy as *mut _ as *mut c_void,
        data_size: mem::size_of_val(&policy),
    };
    unsafe { set_window_composition_attribute(hwnd, &mut data).as_bool() }
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

struct Coordinate {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    icon_size: i32,
    item_size: i32,
}

impl Coordinate {
    fn new(num_apps: i32, icon_size_max: i32, border_size: i32, icon_border: i32) -> Self {
        let monitor_rect = get_moinitor_rect();
        let monitor_width = monitor_rect.right - monitor_rect.left;
        let monitor_height = monitor_rect.bottom - monitor_rect.top;

        let icon_size =
            ((monitor_width - 2 * border_size) / num_apps - icon_border * 2).min(icon_size_max);

        let item_size = icon_size + icon_border * 2;
        let width = item_size * num_apps + border_size * 2;
        let height = item_size + border_size * 2;
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
