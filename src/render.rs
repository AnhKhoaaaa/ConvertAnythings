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
use windows_numerics::Vector2;

use crate::menu::{Menu, Slice};

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
    center_text: IDWriteTextFormat,
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
            let scale = bounds.size as f32 / 280.0;
            let center_text = text_format(&text, 14.0 * scale, false)?;
            let label = text_format(&text, 16.0 * scale, true)?;
            Ok(Self {
                target,
                factory,
                center_text,
                label,
                surface,
                bounds,
            })
        }
    }

    pub fn draw(&self, hwnd: HWND, menu: &Menu) -> Result<()> {
        let radius = self.bounds.size as f32 / 2.0;
        let brush = unsafe {
            self.target
                .CreateSolidColorBrush(&rgb(0xffffff, 1.0), None)?
        };
        unsafe {
            self.target.BeginDraw();
        }
        let draw = self.paint(menu, radius, &brush);
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
            SourceConstantAlpha: 255,
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

    fn paint(&self, menu: &Menu, radius: f32, brush: &ID2D1SolidColorBrush) -> Result<()> {
        unsafe {
            // Nonzero alpha catches DragEnter near screen corners even if the wheel had to move inward.
            self.target.Clear(Some(&rgb(0x000000, 0.008)));
            let disc = ellipse(radius, 0.98);
            brush.SetColor(&rgb(0x0f2026, 0.97));
            self.target.FillEllipse(&disc, brush);
            brush.SetColor(&rgb(0x417679, 1.0));
            self.target
                .DrawEllipse(&disc, brush, 1.2, None::<&ID2D1StrokeStyle>);
            let center = ellipse(radius, 0.35);
            brush.SetColor(&rgb(0x213e43, 1.0));
            self.target.FillEllipse(&center, brush);
            self.text(
                menu.title(),
                &self.center_text,
                brush,
                radius,
                radius,
                radius * 0.32,
                0xdaf0ee,
            );

            for slice in menu.slices() {
                let shape = geometry(&self.factory, slice, radius)?;
                let selected = menu.selected(slice);
                brush.SetColor(&rgb(if selected { 0x81e7d2 } else { 0x294950 }, 1.0));
                self.target.FillGeometry(&shape, brush, None::<&ID2D1Brush>);
                let mid = (slice.inner + slice.outer) / 2.0;
                let label = point(radius, mid, slice.angle);
                self.text(
                    slice.label(),
                    &self.label,
                    brush,
                    label.X,
                    label.Y,
                    radius * 0.38,
                    if selected { 0x102b2b } else { 0xe3f5f3 },
                );
            }
        }
        Ok(())
    }

    fn text(
        &self,
        text: &str,
        style: &IDWriteTextFormat,
        brush: &ID2D1SolidColorBrush,
        x: f32,
        y: f32,
        half_width: f32,
        color: u32,
    ) {
        let text: Vec<u16> = text.encode_utf16().collect();
        let rect = D2D_RECT_F {
            left: x - half_width,
            top: y - 24.0,
            right: x + half_width,
            bottom: y + 24.0,
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

unsafe fn text_format(
    factory: &IDWriteFactory,
    size: f32,
    label: bool,
) -> Result<IDWriteTextFormat> {
    unsafe {
        let style = factory.CreateTextFormat(
            w!("Segoe UI"),
            None::<&IDWriteFontCollection>,
            if label {
                DWRITE_FONT_WEIGHT_SEMI_BOLD
            } else {
                DWRITE_FONT_WEIGHT_NORMAL
            },
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

fn geometry(factory: &ID2D1Factory, slice: Slice, radius: f32) -> Result<ID2D1PathGeometry> {
    let start = slice.angle - slice.half_width;
    let end = slice.angle + slice.half_width;
    let arc_size = if slice.half_width > std::f32::consts::FRAC_PI_2 {
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
        sink.BeginFigure(point(radius, slice.outer, start), D2D1_FIGURE_BEGIN_FILLED);
        sink.AddArc(&arc(end, slice.outer, D2D1_SWEEP_DIRECTION_CLOCKWISE));
        sink.AddLine(point(radius, slice.inner, end));
        sink.AddArc(&arc(
            start,
            slice.inner,
            D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
        ));
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
