//! Right-to-left detection (Feishin's `lyrics-rtl.ts`: Arabic, Hebrew, Syriac, Thaana).

/// Whether `text` contains a right-to-left script character.
pub fn is_rtl(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c as u32,
            0x0590..=0x05FF // Hebrew
            | 0x0600..=0x06FF // Arabic
            | 0x0700..=0x074F // Syriac
            | 0x0750..=0x077F // Arabic supplement
            | 0x0780..=0x07BF // Thaana
            | 0x08A0..=0x08FF // Arabic extended-A
            | 0xFB1D..=0xFB4F // Hebrew presentation forms
            | 0xFB50..=0xFDFF // Arabic presentation forms A
            | 0xFE70..=0xFEFF // Arabic presentation forms B
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_rtl_scripts() {
        assert!(is_rtl("مرحبا"));
        assert!(is_rtl("שלום"));
        assert!(is_rtl("mixed שלום text"));
        assert!(!is_rtl("Hello"));
        assert!(!is_rtl("こんにちは"));
        assert!(!is_rtl(""));
    }
}
