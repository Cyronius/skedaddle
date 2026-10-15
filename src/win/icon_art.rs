type Rgb = (f32, f32, f32);

fn in_round_rect(x: f32, y: f32, l: f32, t: f32, r: f32, b: f32, rad: f32) -> bool {
    if x < l || x > r || y < t || y > b {
        return false;
    }
    let cx = x.clamp(l + rad, r - rad);
    let cy = y.clamp(t + rad, b - rad);
    (x - cx).hypot(y - cy) <= rad
}

const ORANGE: Rgb = (255.0, 122.0, 26.0);
const SHOE: Rgb = (194.0, 65.0, 12.0);
const BLUE: Rgb = (66.0, 133.0, 244.0);
const LEAN: f32 = 0.1745;

fn dist_to_segment(px: f32, py: f32, a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((px - a.0) * dx + (py - a.1) * dy) / (dx * dx + dy * dy).max(1e-6)).clamp(0.0, 1.0);
    (px - (a.0 + t * dx)).hypot(py - (a.1 + t * dy))
}

fn near(px: f32, py: f32, path: &[(f32, f32)], radius: f32) -> bool {
    path.windows(2).any(|s| dist_to_segment(px, py, s[0], s[1]) <= radius)
}

fn sample_page(px: f32, py: f32, detail: bool) -> Option<Rgb> {
    let (dx, dy) = (px - 34.0, py - 20.0);
    let (sin, cos) = LEAN.sin_cos();
    let (x, y) = (34.0 + dx * cos + dy * sin, 20.0 - dx * sin + dy * cos);
    if !in_round_rect(x, y, 12.25, 2.25, 55.75, 37.75, 8.75) {
        return None;
    }
    if !in_round_rect(x, y, 15.75, 5.75, 52.25, 34.25, 5.25) {
        return Some(ORANGE);
    }
    if y < 15.0 {
        return Some(BLUE);
    }
    if detail {
        let in_cell = |cx: f32, top: f32, bottom: f32| x >= cx && x <= cx + 7.0 && y >= top && y <= bottom;
        let top_row = [20.0, 30.0, 40.0].iter().any(|c| in_cell(*c, 21.0, 26.0));
        let low_row = [20.0, 30.0].iter().any(|c| in_cell(*c, 28.0, 32.0));
        if top_row || low_row {
            return Some((156.0, 163.0, 175.0));
        }
    }
    Some((255.0, 255.0, 255.0))
}

fn sample(x: f32, y: f32, stale: bool, detail: bool) -> Option<Rgb> {
    if stale {
        let d = (x - 0.79).hypot(y - 0.21);
        if d < 0.21 {
            return Some(if d < 0.15 { (229.0, 72.0, 77.0) } else { (255.0, 255.0, 255.0) });
        }
    }
    let (px, py) = (x * 64.0, y * 64.0);
    if let Some(c) = sample_page(px, py, detail) {
        return Some(c);
    }
    if near(px, py, &[(15.0, 53.0), (23.0, 56.0)], 4.5) || near(px, py, &[(47.0, 53.0), (56.0, 56.0)], 4.5) {
        return Some(SHOE);
    }
    if near(px, py, &[(26.0, 35.0), (22.0, 46.0), (15.0, 53.0)], 3.25) || near(px, py, &[(38.0, 37.0), (47.0, 43.0), (47.0, 53.0)], 3.25) {
        return Some(ORANGE);
    }
    if near(px, py, &[(3.0, 16.0), (10.0, 16.0)], 1.6) || near(px, py, &[(2.0, 23.0), (11.0, 23.0)], 1.6) || near(px, py, &[(5.0, 30.0), (11.0, 30.0)], 1.6) {
        return Some(BLUE);
    }
    None
}

/// Top-down BGRA pixels, straight (not premultiplied) alpha.
pub fn render_bgra(n: usize, stale: bool) -> Vec<u8> {
    const SS: usize = 4;
    let mut px = vec![0u8; n * n * 4];
    for y in 0..n {
        for x in 0..n {
            let (mut r, mut g, mut b, mut cover) = (0f32, 0f32, 0f32, 0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let fx = (x as f32 + (sx as f32 + 0.5) / SS as f32) / n as f32;
                    let fy = (y as f32 + (sy as f32 + 0.5) / SS as f32) / n as f32;
                    if let Some((cr, cg, cb)) = sample(fx, fy, stale, n >= 24) {
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
    px
}
