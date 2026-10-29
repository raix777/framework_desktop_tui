#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    /// Black: a LED at this color is off.
    pub const BLACK: Color = Color::new(0, 0, 0);

    /// White: the brightest, fully-lit color.
    pub const WHITE: Color = Color::new(255, 255, 255);

    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    pub const fn inverse(self) -> Color {
        Color::new(255 - self.r, 255 - self.g, 255 - self.b)
    }

    /// A fully saturated color for `hue` in degrees: `0` is red, `120` is
    /// green, `240` is blue, and `360` wraps around to red.
    pub fn from_hue(hue: f64) -> Color {
        let (sector, fraction) = Self::hue_sector(hue);
        let (r, g, b) = Self::saturated_rgb(sector, fraction);
        Color::new(
            (r * 255.0).round() as u8,
            (g * 255.0).round() as u8,
            (b * 255.0).round() as u8,
        )
    }

    /// Where `hue` sits on the wheel: which 60° sector, and where in it.
    fn hue_sector(hue: f64) -> (usize, f64) {
        let scaled = hue.rem_euclid(360.0) / 60.0;
        (scaled.floor() as usize % 6, scaled - scaled.floor())
    }

    /// The RGB channels (each `0..=1`) for a sector and its fraction: one
    /// channel full, one ramping between 0 and full, one at zero.
    fn saturated_rgb(sector: usize, fraction: f64) -> (f64, f64, f64) {
        match sector {
            0 => (1.0, fraction, 0.0),
            1 => (1.0 - fraction, 1.0, 0.0),
            2 => (0.0, 1.0, fraction),
            3 => (0.0, 1.0 - fraction, 1.0),
            4 => (fraction, 0.0, 1.0),
            _ => (1.0, 0.0, 1.0 - fraction),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_flips_each_channel() {
        assert_eq!(Color::new(255, 0, 0).inverse(), Color::new(0, 255, 255));
        assert_eq!(Color::new(0, 0, 0).inverse(), Color::new(255, 255, 255));
        assert_eq!(Color::new(128, 200, 10).inverse(), Color::new(127, 55, 245));
    }

    #[test]
    fn from_hue_gives_pure_primary_hues() {
        assert_eq!(Color::from_hue(0.0), Color::new(255, 0, 0));
        assert_eq!(Color::from_hue(120.0), Color::new(0, 255, 0));
        assert_eq!(Color::from_hue(240.0), Color::new(0, 0, 255));
    }

    #[test]
    fn from_hue_wraps_and_blends() {
        assert_eq!(
            Color::from_hue(360.0),
            Color::new(255, 0, 0),
            "360° is red again"
        );
        assert_eq!(Color::from_hue(60.0), Color::new(255, 255, 0), "yellow");
        assert_eq!(Color::from_hue(180.0), Color::new(0, 255, 255), "cyan");
        assert_eq!(Color::from_hue(45.0), Color::new(255, 191, 0), "orange");
        assert_eq!(Color::from_hue(-90.0), Color::new(128, 0, 255), "violet");
    }

}
