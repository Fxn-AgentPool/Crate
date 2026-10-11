//! Material channels. See docs/reference/channels.md.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Channel {
    BaseColor,
    Roughness,
    Metallic,
    Height,
    Normal,
    Emissive,
    Opacity,
}

impl Channel {
    pub const ALL: [Channel; 7] = [
        Channel::BaseColor,
        Channel::Roughness,
        Channel::Metallic,
        Channel::Height,
        Channel::Normal,
        Channel::Emissive,
        Channel::Opacity,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Channel::BaseColor => "Base Color",
            Channel::Roughness => "Roughness",
            Channel::Metallic => "Metallic",
            Channel::Height => "Height",
            Channel::Normal => "Normal",
            Channel::Emissive => "Emissive",
            Channel::Opacity => "Opacity",
        }
    }

    /// WGSL field name in the `Material` struct.
    pub fn field(self) -> &'static str {
        match self {
            Channel::BaseColor => "base_color",
            Channel::Roughness => "roughness",
            Channel::Metallic => "metallic",
            Channel::Height => "height",
            Channel::Normal => "normal",
            Channel::Emissive => "emissive",
            Channel::Opacity => "opacity",
        }
    }

    pub fn components(self) -> usize {
        match self {
            Channel::BaseColor | Channel::Normal | Channel::Emissive => 3,
            _ => 1,
        }
    }

    pub fn default_value(self) -> [f32; 3] {
        match self {
            Channel::BaseColor => [0.5, 0.5, 0.5],
            Channel::Roughness => [0.5; 3],
            Channel::Metallic => [0.0; 3],
            Channel::Height => [0.0; 3],
            Channel::Normal => [0.5, 0.5, 1.0],
            Channel::Emissive => [0.0; 3],
            Channel::Opacity => [1.0; 3],
        }
    }

    pub fn enabled_by_default(self) -> bool {
        !matches!(self, Channel::Emissive | Channel::Opacity)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ChannelSettings {
    pub enabled: [bool; 7],
    /// Stored as in the reference: colors in linear RGB, Normal encoded 0..1.
    pub defaults: [[f32; 3]; 7],
    pub height_scale: f32,
}

impl Default for ChannelSettings {
    fn default() -> Self {
        Self {
            enabled: Channel::ALL.map(Channel::enabled_by_default),
            defaults: Channel::ALL.map(Channel::default_value),
            height_scale: 0.01,
        }
    }
}

impl ChannelSettings {
    pub fn is_enabled(&self, c: Channel) -> bool {
        self.enabled[c.index()]
    }
}
