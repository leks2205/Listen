//! Port of `@mantine/colors-generator` (Feishin's primary palette from the accent).
//! The accent sits at the shade with the closest OKLCH lightness; every shade takes its
//! hue/chroma with the lightness from `LIGHTNESS_MAP`, then is (de)saturated as in
//! chroma-js (`lch.c ± 18 × delta`).

use super::Rgb;

const LIGHTNESS_MAP: [f64; 10] = [0.96, 0.907, 0.805, 0.697, 0.605, 0.547, 0.518, 0.445, 0.395, 0.34];
const CHROMA_MAP: [f64; 10] = [0.32, 0.16, 0.08, 0.04, 0.0, 0.0, 0.04, 0.08, 0.16, 0.32];
/// chroma-js `LAB_CONSTANTS.Kn`.
const KN: f64 = 18.0;

/// The 10 shades (index 0 = lightest).
pub fn generate(color: Rgb) -> [Rgb; 10] {
    let (l0, c0, h0) = oklch(color);
    let base = LIGHTNESS_MAP
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - l0).abs().total_cmp(&(b.1 - l0).abs()))
        .map(|(i, _)| i)
        .unwrap_or(5);
    let mut out = [Rgb(0, 0, 0); 10];
    for (i, &l) in LIGHTNESS_MAP.iter().enumerate() {
        // chroma-js keeps the unclipped value between steps; only `hex()` clamps.
        let rgb = oklch_to_rgb(l, c0, h0);
        let (ll, cc, hh) = lch_from_rgb(rgb);
        let cc = (cc + KN * (CHROMA_MAP[i] - CHROMA_MAP[base])).max(0.0);
        out[i] = to_u8(lch_to_rgb(ll, cc, hh));
    }
    out
}

// ---- sRGB <-> linear -----------------------------------------------------------

fn lin(c: f64) -> f64 {
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn delin(c: f64) -> f64 {
    if c <= 0.003_130_8 { 12.92 * c } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 }
}

fn to_u8(rgb: [f64; 3]) -> Rgb {
    let q = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Rgb(q(rgb[0]), q(rgb[1]), q(rgb[2]))
}

// ---- OKLCH ------------------------------------------------------------------------

fn oklch(c: Rgb) -> (f64, f64, f64) {
    let [r, g, b] = [c.0, c.1, c.2].map(|v| lin(f64::from(v) / 255.0));
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    let ll = 0.210_454_255_3 * l + 0.793_617_785_0 * m - 0.004_072_046_8 * s;
    let aa = 1.977_998_495_1 * l - 2.428_592_205_0 * m + 0.450_593_709_9 * s;
    let bb = 0.025_904_037_1 * l + 0.782_771_766_2 * m - 0.808_675_766_0 * s;
    (ll, aa.hypot(bb), bb.atan2(aa))
}

/// OKLCH → gamma-encoded sRGB (unclamped).
fn oklch_to_rgb(ll: f64, c: f64, h: f64) -> [f64; 3] {
    let (aa, bb) = (c * h.cos(), c * h.sin());
    let l = (ll + 0.396_337_777_4 * aa + 0.215_803_757_3 * bb).powi(3);
    let m = (ll - 0.105_561_345_8 * aa - 0.063_854_172_8 * bb).powi(3);
    let s = (ll - 0.089_484_177_5 * aa - 1.291_485_548_0 * bb).powi(3);
    [
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    ]
    .map(delin)
}

// ---- CIE Lab / LCh (chroma-js constants, D65) ----------------------------------

const XN: f64 = 0.950_470;
const YN: f64 = 1.0;
const ZN: f64 = 1.088_830;
const T0: f64 = 0.137_931_034;
const T1: f64 = 0.206_896_552;
const T2: f64 = 0.128_418_55;
const T3: f64 = 0.008_856_452;

fn lch_from_rgb(rgb: [f64; 3]) -> (f64, f64, f64) {
    let [r, g, b] = rgb.map(|v| lin(v.clamp(0.0, 1.0)));
    let f = |t: f64| if t > T3 { t.cbrt() } else { t / T2 + T0 };
    let x = f((0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / XN);
    let y = f((0.212_672_9 * r + 0.715_152_2 * g + 0.072_175_0 * b) / YN);
    let z = f((0.019_333_9 * r + 0.119_192_0 * g + 0.950_304_1 * b) / ZN);
    let (l, a, bb) = (116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z));
    (l, a.hypot(bb), bb.atan2(a))
}

fn lch_to_rgb(l: f64, c: f64, h: f64) -> [f64; 3] {
    let (a, b) = (c * h.cos(), c * h.sin());
    let y = (l + 16.0) / 116.0;
    let x = y + a / 500.0;
    let z = y - b / 200.0;
    let g = |t: f64| if t > T1 { t * t * t } else { T2 * (t - T0) };
    let (x, y, z) = (XN * g(x), YN * g(y), ZN * g(z));
    [
        3.240_454_2 * x - 1.537_138_5 * y - 0.498_531_4 * z,
        -0.969_266_0 * x + 1.876_010_8 * y + 0.041_556_0 * z,
        0.055_643_4 * x - 0.204_025_9 * y + 1.057_225_2 * z,
    ]
    .map(delin)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Rgb, b: Rgb, tol: i32) -> bool {
        (i32::from(a.0) - i32::from(b.0)).abs() <= tol
            && (i32::from(a.1) - i32::from(b.1)).abs() <= tol
            && (i32::from(a.2) - i32::from(b.2)).abs() <= tol
    }

    #[test]
    fn default_accent_shades() {
        // Feishin default accent rgb(53,116,252) sits at index 4.
        let s = generate(Rgb(53, 116, 252));
        assert!(close(s[4], Rgb(53, 116, 252), 6), "{:?}", s[4]);
        // Approximate expected values (hand-computed reference, see design-tokens.md).
        assert!(close(s[5], Rgb(0x27, 0x64, 0xEA), 12), "shade5 {:?}", s[5]);
        assert!(close(s[6], Rgb(0x1E, 0x5A, 0xE0), 14), "shade6 {:?}", s[6]);
        // Monotonically darker.
        let lum = |c: Rgb| u32::from(c.0) * 3 + u32::from(c.1) * 6 + u32::from(c.2);
        assert!(s.windows(2).all(|w| lum(w[0]) >= lum(w[1])));
    }

    #[test]
    fn roundtrips() {
        let c = Rgb(125, 207, 255);
        let (l, ch, h) = oklch(c);
        assert_eq!(to_u8(oklch_to_rgb(l, ch, h)), c);
        let (l, ch, h) = lch_from_rgb([125.0 / 255.0, 207.0 / 255.0, 1.0]);
        assert!(close(to_u8(lch_to_rgb(l, ch, h)), c, 1));
    }
}
