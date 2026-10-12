use std::ffi::c_void;

use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, HICON, ICONINFO};

type Rgb = (f32, f32, f32);

fn in_round_rect(x: f32, y: f32, l: f32, t: f32, r: f32, b: f32, rad: f32) -> bool {
    if x < l || x > r || y < t || y > b {
        return false;
    }
    let cx = x.clamp(l + rad, r - rad);
    let cy = y.clamp(t + rad, b - rad);
    (x - cx).hypot(y - cy) <= rad
}

fn sample(x: f32, y: f32, stale: bool) -> Option<Rgb> {
    if stale {
        let d = (x - 0.74).hypot(y - 0.74);
        if d < 0.26 {
            return Some(if d < 0.19 { (255.0, 149.0, 0.0) } else { (255.0, 255.0, 255.0) });
        }
    }
    if !in_round_rect(x, y, 0.05, 0.09, 0.95, 0.95, 0.15) {
        return None;
    }
    if !in_round_rect(x, y, 0.09, 0.13, 0.91, 0.91, 0.12) {
        return Some((150.0, 156.0, 165.0));
    }
    if y < 0.34 {
        return Some((66.0, 133.0, 244.0));
    }
    let gx = (x - 0.19) / 0.62;
    let gy = (y - 0.44) / 0.40;
    if (0.0..1.0).contains(&gx) && (0.0..1.0).contains(&gy) {
        let (cx, cy) = (gx * 4.0, gy * 3.0);
        if cx.fract() < 0.62 && cy.fract() < 0.55 {
            return Some((95.0, 99.0, 104.0));
        }
    }
    Some((255.0, 255.0, 255.0))
}

pub fn make(size: i32, stale: bool) -> HICON {
    const SS: usize = 4;
    let n = size as usize;
    let mut px = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (mut r, mut g, mut b, mut cover) = (0f32, 0f32, 0f32, 0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = (x as f32 + (sx as f32 + 0.5) / SS as f32) / n as f32;
                    let fy = (y as f32 + (sy as f32 + 0.5) / SS as f32) / n as f32;
                    if let Some((cr, cg, cb)) = sample(fx, fy, stale) {
                        r += cr;
                        g += cg;
                        b += cb;
                        cover += 1.0;
                    }
                }
            }
            if cover > 0.0 {
                let i = (y * n + x) * 4;
                px[i] = (b / cover) as u8;
                px[i + 1] = (g / cover) as u8;
                px[i + 2] = (r / cover) as u8;
                px[i + 3] = (255.0 * cover / (SS * SS) as f32) as u8;
            }
        }
    }
    unsafe {
        let hdc = GetDC(None);
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: size,
                biHeight: -size,
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let color = CreateDIBSection(Some(hdc), &bmi, DIB_RGB_COLORS, &mut bits, None, 0).unwrap_or_default();
        if !bits.is_null() {
            std::ptr::copy_nonoverlapping(px.as_ptr(), bits as *mut u8, px.len());
        }
        let mask = CreateBitmap(size, size, 1, 1, None);
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mask, hbmColor: color };
        let icon = CreateIconIndirect(&info).unwrap_or_default();
        let _ = DeleteObject(HGDIOBJ(color.0));
        let _ = DeleteObject(HGDIOBJ(mask.0));
        ReleaseDC(None, hdc);
        icon
    }
}
