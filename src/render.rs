use std::ffi::c_void;
use std::ptr;

use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::*;
use windows::Win32::Graphics::Direct2D::*;
use windows::Win32::Graphics::DirectWrite::*;
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{ULW_ALPHA, UpdateLayeredWindow};
use windows::core::{Error, Result, w};
use windows_numerics::{Matrix3x2, Vector2};

use crate::menu::{Menu, Slice};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Theme {
    #[default]
    Tangerine,
    Obsidian,
}

impl Theme {
    pub const OPTIONS: [Self; 2] = [Self::Tangerine, Self::Obsidian];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Tangerine => "tangerine",
            Self::Obsidian => "obsidian",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Tangerine => "Tangerine Glass (Sáng)",
            Self::Obsidian => "Obsidian Glass (Tối)",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "tangerine" => Some(Self::Tangerine),
            "obsidian" => Some(Self::Obsidian),
            _ => None,
        }
    }
}

struct Palette {
    shadow: u32,
    disc_top: u32,
    disc_top_a: f32,
    disc_bot: u32,
    disc_bot_a: f32,
    disc_inner_rim: u32,
    disc_inner_rim_a: f32,
    disc_outer_rim: u32,
    disc_outer_rim_a: f32,
    petal_bg: u32,
    petal_bg_a: f32,
    petal_border: u32,
    petal_border_a: f32,
    petal_text: u32,
    active_top: u32,
    active_bot: u32,
    active_border: u32,
    active_text: u32,
    hub_bg: u32,
    hub_bg_a: f32,
    hub_border: u32,
    hub_border_a: f32,
    hub_title: u32,
    hub_title_active: u32,
    hub_sub: u32,
}

const TANGERINE_PALETTE: Palette = Palette {
    shadow: 0x24130a,
    disc_top: 0xfbf6f1,
    disc_top_a: 0.93,
    disc_bot: 0xe9ddd2,
    disc_bot_a: 0.90,
    disc_inner_rim: 0xffffff,
    disc_inner_rim_a: 0.86,
    disc_outer_rim: 0x7c5c46,
    disc_outer_rim_a: 0.22,
    petal_bg: 0xfffaf6,
    petal_bg_a: 0.84,
    petal_border: 0xc2a18a,
    petal_border_a: 0.34,
    petal_text: 0x241f1c,
    active_top: 0xff763f,
    active_bot: 0xe64415,
    active_border: 0xffa67e,
    active_text: 0xffffff,
    hub_bg: 0xffffff,
    hub_bg_a: 0.95,
    hub_border: 0xd9c5b6,
    hub_border_a: 0.72,
    hub_title: 0x231d19,
    hub_title_active: 0xe64415,
    hub_sub: 0x7d6b61,
};

const OBSIDIAN_PALETTE: Palette = Palette {
    shadow: 0x050608,
    disc_top: 0x20232c,
    disc_top_a: 0.94,
    disc_bot: 0x13151b,
    disc_bot_a: 0.93,
    disc_inner_rim: 0xffffff,
    disc_inner_rim_a: 0.16,
    disc_outer_rim: 0x000000,
    disc_outer_rim_a: 0.45,
    petal_bg: 0x2b2f3a,
    petal_bg_a: 0.88,
    petal_border: 0xffffff,
    petal_border_a: 0.10,
    petal_text: 0xf2f4f8,
    active_top: 0xff763f,
    active_bot: 0xe64415,
    active_border: 0xffa67e,
    active_text: 0xffffff,
    hub_bg: 0x1c1f27,
    hub_bg_a: 0.96,
    hub_border: 0xffffff,
    hub_border_a: 0.14,
    hub_title: 0xf4f6fa,
    hub_title_active: 0xff7c48,
    hub_sub: 0x9aa3b2,
};

#[derive(Clone, Copy)]
pub struct Bounds {
    pub x: i32,
    pub y: i32,
    pub size: i32,
}

impl Bounds {
    pub fn relative(self, x: i32, y: i32) -> (f32, f32) {
        let radius = self.size as f32 / 2.0;
        (
            (x - self.x) as f32 / radius - 1.0,
            (y - self.y) as f32 / radius - 1.0,
        )
    }
}

pub struct Renderer {
    target: ID2D1DCRenderTarget,
    factory: ID2D1Factory,
    center_title: IDWriteTextFormat,
    center_sub: IDWriteTextFormat,
    label: IDWriteTextFormat,
    surface: Surface,
    pub bounds: Bounds,
}

impl Renderer {
    pub fn new(bounds: Bounds) -> Result<Self> {
        unsafe {
            let surface = Surface::new(bounds.size)?;
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None)?;
            let properties = D2D1_RENDER_TARGET_PROPERTIES {
                r#type: D2D1_RENDER_TARGET_TYPE_SOFTWARE,
                pixelFormat: D2D1_PIXEL_FORMAT {
                    format: DXGI_FORMAT_B8G8R8A8_UNORM,
                    alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED,
                },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            let target = factory.CreateDCRenderTarget(&properties)?;
            target.BindDC(
                surface.dc,
                &RECT {
                    left: 0,
                    top: 0,
                    right: bounds.size,
                    bottom: bounds.size,
                },
            )?;
            target.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            let text: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_ISOLATED)?;
            let scale = bounds.size as f32 / 320.0;
            let center_title = text_format(&text, 15.0 * scale, DWRITE_FONT_WEIGHT_BOLD)?;
            let center_sub = text_format(&text, 11.0 * scale, DWRITE_FONT_WEIGHT_MEDIUM)?;
            let label = text_format(&text, 14.0 * scale, DWRITE_FONT_WEIGHT_SEMI_BOLD)?;
            Ok(Self {
                target,
                factory,
                center_title,
                center_sub,
                label,
                surface,
                bounds,
            })
        }
    }

    pub fn draw(
        &self,
        hwnd: HWND,
        menu: &Menu,
        theme: Theme,
        anim_scale: f32,
        anim_alpha: f32,
    ) -> Result<()> {
        let radius = self.bounds.size as f32 / 2.0;
        let brush = unsafe {
            self.target
                .CreateSolidColorBrush(&rgb(0xffffff, 1.0), None)?
        };
        unsafe {
            self.target.BeginDraw();
            let s = anim_scale.clamp(0.5, 1.05);
            let m = Matrix3x2 {
                M11: s,
                M12: 0.0,
                M21: 0.0,
                M22: s,
                M31: radius * (1.0 - s),
                M32: radius * (1.0 - s),
            };
            self.target.SetTransform(&m);
        }
        let draw = self.paint(menu, radius, &brush, theme);
        unsafe {
            self.target.SetTransform(&Matrix3x2::identity());
        }
        let end = unsafe { self.target.EndDraw(None, None) };
        draw.and(end)?;

        let from = POINT::default();
        let to = POINT {
            x: self.bounds.x,
            y: self.bounds.y,
        };
        let size = SIZE {
            cx: self.bounds.size,
            cy: self.bounds.size,
        };
        let alpha = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: (anim_alpha.clamp(0.0, 1.0) * 255.0).round() as u8,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        unsafe {
            UpdateLayeredWindow(
                hwnd,
                None,
                Some(&to),
                Some(&size),
                Some(self.surface.dc),
                Some(&from),
                COLORREF(0),
                Some(&alpha),
                ULW_ALPHA,
            )
        }
    }

    fn paint(
        &self,
        menu: &Menu,
        radius: f32,
        brush: &ID2D1SolidColorBrush,
        theme: Theme,
    ) -> Result<()> {
        let p = match theme {
            Theme::Tangerine => &TANGERINE_PALETTE,
            Theme::Obsidian => &OBSIDIAN_PALETTE,
        };
        let px = radius / 160.0;
        unsafe {
            // Nonzero alpha catches DragEnter near screen corners even if the wheel had to move inward.
            self.target.Clear(Some(&rgb(0x000000, 0.008)));

            // Multi-ring soft ambient shadow
            for (ratio, alpha) in [(0.988, 0.055), (0.974, 0.085), (0.960, 0.11)] {
                let mut shadow_disc = ellipse(radius, ratio);
                shadow_disc.point.Y += 2.5 * px;
                brush.SetColor(&rgb(p.shadow, alpha));
                self.target.FillEllipse(&shadow_disc, brush);
            }

            // Frosted glass base disc with subtle radial lighting
            let disc = ellipse(radius, 0.945);
            let disc_brush = radial_brush(
                &self.target,
                Vector2 {
                    X: radius * 0.72,
                    Y: radius * 0.65,
                },
                radius * 1.35,
                p.disc_top,
                p.disc_top_a,
                p.disc_bot,
                p.disc_bot_a,
            )?;
            self.target.FillEllipse(&disc, &disc_brush);

            // Inner specular rim + outer hairline
            let inner_rim = ellipse(radius, 0.938);
            brush.SetColor(&rgb(p.disc_inner_rim, p.disc_inner_rim_a));
            self.target
                .DrawEllipse(&inner_rim, brush, 1.4 * px, None::<&ID2D1StrokeStyle>);
            brush.SetColor(&rgb(p.disc_outer_rim, p.disc_outer_rim_a));
            self.target
                .DrawEllipse(&disc, brush, 1.0 * px, None::<&ID2D1StrokeStyle>);

            // Petals
            let active_brush = linear_brush(
                &self.target,
                Vector2 {
                    X: radius * 0.4,
                    Y: radius * 0.25,
                },
                Vector2 {
                    X: radius * 1.6,
                    Y: radius * 1.75,
                },
                p.active_top,
                1.0,
                p.active_bot,
                1.0,
            )?;

            for slice in menu.slices() {
                let selected = menu.selected(slice);
                let shape = rounded_petal_geometry(&self.factory, slice, radius, selected)?;
                if selected {
                    self.target
                        .FillGeometry(&shape, &active_brush, None::<&ID2D1Brush>);
                    brush.SetColor(&rgb(p.active_border, 0.7));
                    self.target.DrawGeometry(
                        &shape,
                        brush,
                        1.2 * px,
                        None::<&ID2D1StrokeStyle>,
                    );
                } else {
                    brush.SetColor(&rgb(p.petal_bg, p.petal_bg_a));
                    self.target.FillGeometry(&shape, brush, None::<&ID2D1Brush>);
                    brush.SetColor(&rgb(p.petal_border, p.petal_border_a));
                    self.target.DrawGeometry(
                        &shape,
                        brush,
                        1.0 * px,
                        None::<&ID2D1StrokeStyle>,
                    );
                }

                let mid = (slice.inner + slice.outer) / 2.0 + if selected { 0.008 } else { 0.0 };
                let label = point(radius, mid, slice.angle);
                self.text(
                    slice.label(),
                    &self.label,
                    brush,
                    label,
                    radius * 0.34,
                    18.0 * px,
                    if selected { p.active_text } else { p.petal_text },
                );
            }

            // Frosted center hub
            let center = ellipse(radius, 0.335);
            brush.SetColor(&rgb(p.hub_bg, p.hub_bg_a));
            self.target.FillEllipse(&center, brush);
            brush.SetColor(&rgb(p.hub_border, p.hub_border_a));
            self.target
                .DrawEllipse(&center, brush, 1.2 * px, None::<&ID2D1StrokeStyle>);

            let subtitle = menu.subtitle();
            if subtitle.is_empty() {
                self.text(
                    menu.title(),
                    &self.center_title,
                    brush,
                    center.point,
                    radius * 0.29,
                    24.0 * px,
                    if menu.has_selection() {
                        p.hub_title_active
                    } else {
                        p.hub_title
                    },
                );
            } else {
                self.text(
                    menu.title(),
                    &self.center_title,
                    brush,
                    Vector2 {
                        X: center.point.X,
                        Y: center.point.Y - 9.0 * px,
                    },
                    radius * 0.29,
                    18.0 * px,
                    if menu.has_selection() {
                        p.hub_title_active
                    } else {
                        p.hub_title
                    },
                );
                self.text(
                    subtitle,
                    &self.center_sub,
                    brush,
                    Vector2 {
                        X: center.point.X,
                        Y: center.point.Y + 12.0 * px,
                    },
                    radius * 0.29,
                    14.0 * px,
                    p.hub_sub,
                );
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &self,
        text: &str,
        style: &IDWriteTextFormat,
        brush: &ID2D1SolidColorBrush,
        center: Vector2,
        half_width: f32,
        half_height: f32,
        color: u32,
    ) {
        let text: Vec<u16> = text.encode_utf16().collect();
        let rect = D2D_RECT_F {
            left: center.X - half_width,
            top: center.Y - half_height,
            right: center.X + half_width,
            bottom: center.Y + half_height,
        };
        unsafe {
            brush.SetColor(&rgb(color, 1.0));
            self.target.DrawText(
                &text,
                style,
                &rect,
                brush,
                D2D1_DRAW_TEXT_OPTIONS_CLIP,
                DWRITE_MEASURING_MODE_NATURAL,
            );
        }
    }
}

unsafe fn radial_brush(
    target: &ID2D1DCRenderTarget,
    center: Vector2,
    radius: f32,
    c0: u32,
    a0: f32,
    c1: u32,
    a1: f32,
) -> Result<ID2D1RadialGradientBrush> {
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: rgb(c0, a0),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgb(c1, a1),
        },
    ];
    unsafe {
        let collection = target.CreateGradientStopCollection(
            &stops,
            D2D1_GAMMA_2_2,
            D2D1_EXTEND_MODE_CLAMP,
        )?;
        target.CreateRadialGradientBrush(
            &D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES {
                center,
                gradientOriginOffset: Vector2::default(),
                radiusX: radius,
                radiusY: radius,
            },
            None,
            &collection,
        )
    }
}

unsafe fn linear_brush(
    target: &ID2D1DCRenderTarget,
    start: Vector2,
    end: Vector2,
    c0: u32,
    a0: f32,
    c1: u32,
    a1: f32,
) -> Result<ID2D1LinearGradientBrush> {
    let stops = [
        D2D1_GRADIENT_STOP {
            position: 0.0,
            color: rgb(c0, a0),
        },
        D2D1_GRADIENT_STOP {
            position: 1.0,
            color: rgb(c1, a1),
        },
    ];
    unsafe {
        let collection = target.CreateGradientStopCollection(
            &stops,
            D2D1_GAMMA_2_2,
            D2D1_EXTEND_MODE_CLAMP,
        )?;
        target.CreateLinearGradientBrush(
            &D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                startPoint: start,
                endPoint: end,
            },
            None,
            &collection,
        )
    }
}

unsafe fn text_format(
    factory: &IDWriteFactory,
    size: f32,
    weight: DWRITE_FONT_WEIGHT,
) -> Result<IDWriteTextFormat> {
    unsafe {
        let style = factory.CreateTextFormat(
            w!("Segoe UI"),
            None::<&IDWriteFontCollection>,
            weight,
            DWRITE_FONT_STYLE_NORMAL,
            DWRITE_FONT_STRETCH_NORMAL,
            size,
            w!("vi-VN"),
        )?;
        style.SetTextAlignment(DWRITE_TEXT_ALIGNMENT_CENTER)?;
        style.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER)?;
        Ok(style)
    }
}

fn rgb(hex: u32, alpha: f32) -> D2D1_COLOR_F {
    let component = |shift: u32| ((hex >> shift) & 0xff) as f32 / 255.0;
    D2D1_COLOR_F {
        r: component(16),
        g: component(8),
        b: component(0),
        a: alpha,
    }
}

fn ellipse(radius: f32, ratio: f32) -> D2D1_ELLIPSE {
    D2D1_ELLIPSE {
        point: Vector2 {
            X: radius,
            Y: radius,
        },
        radiusX: ratio * radius,
        radiusY: ratio * radius,
    }
}

fn point(radius: f32, distance: f32, angle: f32) -> Vector2 {
    Vector2 {
        X: radius * (1.0 + distance * angle.cos()),
        Y: radius * (1.0 + distance * angle.sin()),
    }
}

fn rounded_petal_geometry(
    factory: &ID2D1Factory,
    slice: Slice,
    radius: f32,
    selected: bool,
) -> Result<ID2D1PathGeometry> {
    let inner = if selected {
        slice.inner - 0.006
    } else {
        slice.inner
    };
    let outer = if selected {
        slice.outer + 0.024
    } else {
        slice.outer
    };
    let r_in = inner * radius;
    let r_out = outer * radius;
    let hw = slice.half_width;

    // Corner rounding in pixels, clamped to keep arcs well-formed even for narrow slices
    let max_cr = ((r_out - r_in) * 0.28).min(r_in * hw * 0.55);
    let cr = (11.0 * (radius / 160.0)).min(max_cr).max(1.5);
    let d_out = (cr / r_out).min(hw * 0.45);
    let d_in = (cr / r_in).min(hw * 0.45);
    let r_out_inset = outer - cr / radius;
    let r_in_inset = inner + cr / radius;

    let start = slice.angle - hw;
    let end = slice.angle + hw;
    let arc_size = if (hw - d_out) > std::f32::consts::FRAC_PI_2 {
        D2D1_ARC_SIZE_LARGE
    } else {
        D2D1_ARC_SIZE_SMALL
    };
    let arc = |at, distance, sweep| D2D1_ARC_SEGMENT {
        point: point(radius, distance, at),
        size: D2D_SIZE_F {
            width: radius * distance,
            height: radius * distance,
        },
        rotationAngle: 0.0,
        sweepDirection: sweep,
        arcSize: arc_size,
    };

    unsafe {
        let shape = factory.CreatePathGeometry()?;
        let sink = shape.Open()?;
        sink.BeginFigure(
            point(radius, r_out_inset, start),
            D2D1_FIGURE_BEGIN_FILLED,
        );
        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
            point1: point(radius, outer, start),
            point2: point(radius, outer, start + d_out),
        });
        sink.AddArc(&arc(
            end - d_out,
            outer,
            D2D1_SWEEP_DIRECTION_CLOCKWISE,
        ));
        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
            point1: point(radius, outer, end),
            point2: point(radius, r_out_inset, end),
        });
        sink.AddLine(point(radius, r_in_inset, end));
        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
            point1: point(radius, inner, end),
            point2: point(radius, inner, end - d_in),
        });
        sink.AddArc(&arc(
            start + d_in,
            inner,
            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
        ));
        sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT {
            point1: point(radius, inner, start),
            point2: point(radius, r_in_inset, start),
        });
        sink.EndFigure(D2D1_FIGURE_END_CLOSED);
        sink.Close()?;
        Ok(shape)
    }
}

struct Surface {
    dc: HDC,
    bitmap: HBITMAP,
    original: HGDIOBJ,
}

impl Surface {
    unsafe fn new(size: i32) -> Result<Self> {
        unsafe {
            let dc = CreateCompatibleDC(None);
            if dc.is_invalid() {
                return Err(Error::from_thread());
            }
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: size,
                    biHeight: -size,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels: *mut c_void = ptr::null_mut();
            let bitmap =
                match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut pixels, None, 0) {
                    Ok(bitmap) => bitmap,
                    Err(error) => {
                        let _ = DeleteDC(dc);
                        return Err(error);
                    }
                };
            let original = SelectObject(dc, HGDIOBJ(bitmap.0));
            if original.is_invalid() {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(dc);
                return Err(Error::from_thread());
            }
            Ok(Self {
                dc,
                bitmap,
                original,
            })
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.original);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.dc);
        }
    }
}
