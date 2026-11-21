// The application icon, drawn in code so the window (taskbar) and the .exe use the same
// picture: a blue rounded square with a white branch graph. Shared with build.rs via include!.

/// RGBA pixels of the icon at `size` x `size`.
pub fn icon_pixels(size: u32) -> Vec<u8> {
    let s = size as f32;
    let mut px = vec![0u8; (size * size * 4) as usize];
    // distance from point to segment
    fn seg(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        let (cx, cy) = (ax + t * dx, ay + t * dy);
        ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
    }
    for y in 0..size {
        for x in 0..size {
            let fx = (x as f32 + 0.5) / s;
            let fy = (y as f32 + 0.5) / s;
            // rounded square (corner radius 0.22, margin 0.04)
            let m = 0.04;
            let r = 0.22;
            let qx = (fx - 0.5).abs() - (0.5 - m - r);
            let qy = (fy - 0.5).abs() - (0.5 - m - r);
            let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
            let a_bg = (0.5 - outside * s).clamp(0.0, 1.0);
            if a_bg <= 0.0 {
                continue;
            }
            // vertical gradient
            let t = fy;
            let (r0, g0, b0) = (0x3d as f32, 0x8e as f32, 0xe6 as f32);
            let (r1, g1, b1) = (0x22 as f32, 0x5c as f32, 0xa8 as f32);
            let mut col = [r0 + (r1 - r0) * t, g0 + (g1 - g0) * t, b0 + (b1 - b0) * t];
            // branch graph: main line, side branch curve (as two segments), three dots
            let w = 0.055;
            let mut d = seg(fx, fy, 0.36, 0.27, 0.36, 0.73);
            d = d.min(seg(fx, fy, 0.66, 0.34, 0.66, 0.44));
            d = d.min(seg(fx, fy, 0.66, 0.44, 0.36, 0.62));
            let mut dot: f32 = 10.0;
            for (cx, cy) in [(0.36, 0.27), (0.36, 0.73), (0.66, 0.32)] {
                dot = dot.min(((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() - 0.095);
            }
            let line = d - w / 2.0;
            let shape = line.min(dot);
            let a_fg = (0.5 - shape * s).clamp(0.0, 1.0);
            for c in col.iter_mut() {
                *c = *c * (1.0 - a_fg) + 255.0 * a_fg;
            }
            let i = ((y * size + x) * 4) as usize;
            px[i] = col[0] as u8;
            px[i + 1] = col[1] as u8;
            px[i + 2] = col[2] as u8;
            px[i + 3] = (a_bg * 255.0) as u8;
        }
    }
    px
}
