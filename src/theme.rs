pub const AUTO: &str = "auto";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Light,
    Midtone,
    Dark,
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub id: &'static str,
    pub label: &'static str,
    pub tone: Tone,
    pub font: &'static str,
    pub bg: u32,
    pub panel: u32,
    pub line: u32,
    pub grid: u32,
    pub text: u32,
    pub dim: u32,
    pub accent: u32,
    pub ok: u32,
    pub err: u32,
}

const fn t(id: &'static str, label: &'static str, tone: Tone, font: &'static str, c: [u32; 9]) -> Theme {
    Theme { id, label, tone, font, bg: c[0], panel: c[1], line: c[2], grid: c[3], text: c[4], dim: c[5], accent: c[6], ok: c[7], err: c[8] }
}

pub const THEMES: [Theme; 18] = [
    t("light", "Basic Light", Tone::Light, "Segoe UI", [0xFFFFFF, 0xF4F5F7, 0xE1E3E7, 0xE1E3E7, 0x1F2328, 0x667085, 0x1A73E8, 0x1E8E3E, 0xEA4335]),
    t("tater", "Tater Dog Light", Tone::Light, "Candara", [0xFFFFFF, 0xD6ECDD, 0x2A2620, 0xCFE0D4, 0x1A1714, 0x594531, 0x7A4424, 0x3F7D4A, 0xC0432B]),
    t("solarized-light", "Solarized Light", Tone::Light, "Sitka Text", [0xFDF6E3, 0xE7E0CC, 0xCFC8AD, 0xE0D9C2, 0x073642, 0x4E5F65, 0xB58900, 0x859900, 0xDC322F]),
    t("mint-ledger", "Mint Ledger", Tone::Light, "Calibri", [0xF2F7F3, 0xDFE9E2, 0xC3D3C8, 0xDBE6DF, 0x1F2A24, 0x54635A, 0x2F8F5F, 0x3F9A5F, 0xC0503F]),
    t("cozy", "Cozy Reading Room", Tone::Light, "Constantia", [0xF6EFE3, 0xFDF6EA, 0xE3D6C0, 0xEDE2CF, 0x2A1F17, 0x735D4D, 0xB8553A, 0x5F7D3A, 0xB5482F]),
    t("jadeite", "Jadeite Diner", Tone::Light, "Franklin Gothic Medium", [0xE2F2E8, 0xC1DBCE, 0x8CB8A2, 0xBAD7C8, 0x17332B, 0x4E6B61, 0xD92F4B, 0x2F8A5F, 0xC02838]),
    t("terracotta-dusk", "Terracotta Dusk", Tone::Midtone, "Georgia", [0x4D4139, 0x594C42, 0x6B5B4D, 0x524438, 0xF7EDE2, 0xDAD1C8, 0xE2793F, 0x8FBF6A, 0xE2705F]),
    t("concrete-violet", "Concrete Violet", Tone::Midtone, "Trebuchet MS", [0x45414D, 0x504C59, 0x665F74, 0x4D495A, 0xF1EFF6, 0xD2CFD9, 0xA97FE0, 0x7FC98A, 0xE2776A]),
    t("rosewood", "Rosewood", Tone::Midtone, "Palatino Linotype", [0x483A3E, 0x544349, 0x6B535A, 0x4F4046, 0xF5E9EC, 0xCEC0C3, 0xC25A75, 0x8FBF6A, 0xD9705F]),
    t("canyon", "Canyon", Tone::Midtone, "Sitka Heading", [0x66473A, 0x735244, 0x8E6B5A, 0x6A4B3D, 0xFAEEE5, 0xE3D3C6, 0x5FB8E8, 0x93BF6A, 0xE05A45]),
    t("periwinkle-dusk", "Periwinkle Dusk", Tone::Midtone, "Corbel", [0x50556F, 0x5B617F, 0x777DA0, 0x565C78, 0xF2F3FA, 0xD5D8E8, 0xFFB08A, 0x8FD49F, 0xE87A7A]),
    t("cyberdeck", "Cyberdeck", Tone::Midtone, "Consolas", [0x38414B, 0x424C58, 0x5F6D7D, 0x3E4854, 0xEFF5FA, 0xCCD8E2, 0xC6F24A, 0x5FE0A0, 0xFF6F9A]),
    t("dark", "Dark", Tone::Dark, "Segoe UI", [0x1E1F22, 0x2B2D30, 0x3A3C40, 0x3A3C40, 0xE6E6E6, 0x9AA0A6, 0x5B9BFF, 0x34A853, 0xFF6B5E]),
    t("chillwave", "Chill Wave", Tone::Dark, "Segoe UI Black", [0x1A0B2E, 0x241041, 0x3D2352, 0x251142, 0xF3E6FF, 0x9E86C1, 0xFF6EC7, 0x4FE0C0, 0xFF5C8A]),
    t("phosphor", "Phosphor", Tone::Dark, "Lucida Console", [0x0A0F0C, 0x0F1712, 0x1E2B22, 0x14201A, 0xD6EAD9, 0x6F9078, 0x3BD16F, 0x63D68A, 0xE2685F]),
    t("solarized-dark", "Solarized Dark", Tone::Dark, "Sitka Text", [0x002B36, 0x073642, 0x0F4B5A, 0x073642, 0xEEE8D5, 0xA7B3B4, 0xB58900, 0x859900, 0xDC322F]),
    t("copper-slate", "Copper Slate", Tone::Dark, "Bahnschrift", [0x1C1F22, 0x23272B, 0x333A40, 0x262B30, 0xE6E8EA, 0x93999F, 0xD08A5A, 0x7FB389, 0xDB6F64]),
    t("ledger", "Ledger", Tone::Dark, "Cambria", [0x15181D, 0x1A1E24, 0x272D36, 0x1F242C, 0xE9E6DE, 0x8E939D, 0xD2A24C, 0x6FB47C, 0xE0705F]),
];

impl Theme {
    pub fn is_dark(&self) -> bool {
        luminance(self.bg) < 0.4
    }

    pub fn on_accent(&self) -> u32 {
        readable_on(self.accent)
    }

    pub fn on_ok(&self) -> u32 {
        readable_on(self.ok)
    }
}

pub fn find(id: &str) -> Option<&'static Theme> {
    THEMES.iter().find(|t| t.id.eq_ignore_ascii_case(id))
}

pub fn is_known(id: &str) -> bool {
    id.eq_ignore_ascii_case(AUTO) || find(id).is_some()
}

pub fn resolve(id: &str, system_dark: bool) -> &'static Theme {
    let fallback = if system_dark { "dark" } else { "light" };
    find(id).or_else(|| find(fallback)).unwrap_or(&THEMES[0])
}

pub fn luminance(hex: u32) -> f32 {
    let ch = |shift: u32| {
        let c = ((hex >> shift) & 0xFF) as f32 / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
}

pub fn contrast(a: u32, b: u32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

pub fn readable_on(bg: u32) -> u32 {
    if contrast(0xFFFFFF, bg) >= contrast(0x101214, bg) {
        0xFFFFFF
    } else {
        0x101214
    }
}
