//! Remappable keyboard shortcuts. See docs/reference/keyboard-shortcuts.md.

use std::collections::BTreeMap;

use egui::{Key, KeyboardShortcut, Modifiers};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum Action {
    NewProject,
    OpenProject,
    Save,
    ExportTextures,
    Undo,
    Redo,
    ToolPaint,
    ToolErase,
    ToolPaintThrough,
    SmallerBrush,
    LargerBrush,
    SofterBrush,
    HarderBrush,
    Bake,
    ToggleBakeMode,
    ClearBuffer,
    NewPaintLayer,
    NewFillLayer,
    Group,
    Duplicate,
    Delete,
    EditAsGraph,
    ToggleNodeEditor,
    FrameMesh,
    FrameCursor,
    MaterialView,
    CycleChannels,
    CycleMeshMaps,
    TileGrid,
    Wireframe,
    NextStencil,
    SearchLibrary,
    /// Held while dragging to transform the stencil (tap toggles visibility).
    StencilKey,
    /// Held while dragging to transform the paint buffer.
    BufferKey,
}

impl Action {
    pub const ALL: [Action; 34] = [
        Action::NewProject,
        Action::OpenProject,
        Action::Save,
        Action::ExportTextures,
        Action::Undo,
        Action::Redo,
        Action::ToolPaint,
        Action::ToolErase,
        Action::ToolPaintThrough,
        Action::SmallerBrush,
        Action::LargerBrush,
        Action::SofterBrush,
        Action::HarderBrush,
        Action::Bake,
        Action::ToggleBakeMode,
        Action::ClearBuffer,
        Action::NewPaintLayer,
        Action::NewFillLayer,
        Action::Group,
        Action::Duplicate,
        Action::Delete,
        Action::EditAsGraph,
        Action::ToggleNodeEditor,
        Action::FrameMesh,
        Action::FrameCursor,
        Action::MaterialView,
        Action::CycleChannels,
        Action::CycleMeshMaps,
        Action::TileGrid,
        Action::Wireframe,
        Action::NextStencil,
        Action::SearchLibrary,
        Action::StencilKey,
        Action::BufferKey,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Action::NewProject => "New project",
            Action::OpenProject => "Open project",
            Action::Save => "Save",
            Action::ExportTextures => "Export textures",
            Action::Undo => "Undo",
            Action::Redo => "Redo",
            Action::ToolPaint => "Paint tool",
            Action::ToolErase => "Erase tool",
            Action::ToolPaintThrough => "Paint Through tool",
            Action::SmallerBrush => "Smaller brush",
            Action::LargerBrush => "Larger brush",
            Action::SofterBrush => "Softer brush",
            Action::HarderBrush => "Harder brush",
            Action::Bake => "Bake buffer",
            Action::ToggleBakeMode => "Toggle Live / Manual",
            Action::ClearBuffer => "Clear buffer",
            Action::NewPaintLayer => "New paint layer",
            Action::NewFillLayer => "New fill layer",
            Action::Group => "Group selected",
            Action::Duplicate => "Duplicate",
            Action::Delete => "Delete",
            Action::EditAsGraph => "Edit as graph",
            Action::ToggleNodeEditor => "Open or close the Node Editor",
            Action::FrameMesh => "Frame mesh",
            Action::FrameCursor => "Frame cursor",
            Action::MaterialView => "Material view",
            Action::CycleChannels => "Cycle channels",
            Action::CycleMeshMaps => "Cycle mesh maps",
            Action::TileGrid => "Toggle UDIM tile grid",
            Action::Wireframe => "Toggle wireframe",
            Action::NextStencil => "Next stencil",
            Action::SearchLibrary => "Search the library",
            Action::StencilKey => "Stencil transform (hold)",
            Action::BufferKey => "Buffer transform (hold)",
        }
    }

    pub fn default_shortcut(self) -> KeyboardShortcut {
        let c = Modifiers::COMMAND;
        let cs = Modifiers::COMMAND | Modifiers::SHIFT;
        let s = Modifiers::SHIFT;
        let n = Modifiers::NONE;
        let (m, k) = match self {
            Action::NewProject => (c, Key::N),
            Action::OpenProject => (c, Key::O),
            Action::Save => (c, Key::S),
            Action::ExportTextures => (c, Key::E),
            Action::Undo => (c, Key::Z),
            Action::Redo => (cs, Key::Z),
            Action::ToolPaint => (n, Key::B),
            Action::ToolErase => (n, Key::E),
            Action::ToolPaintThrough => (n, Key::P),
            Action::SmallerBrush => (n, Key::OpenBracket),
            Action::LargerBrush => (n, Key::CloseBracket),
            Action::SofterBrush => (s, Key::OpenBracket),
            Action::HarderBrush => (s, Key::CloseBracket),
            Action::Bake => (c, Key::B),
            Action::ToggleBakeMode => (cs, Key::B),
            Action::ClearBuffer => (c, Key::Delete),
            Action::NewPaintLayer => (cs, Key::N),
            Action::NewFillLayer => (cs, Key::F),
            Action::Group => (c, Key::G),
            Action::Duplicate => (c, Key::D),
            Action::Delete => (n, Key::Delete),
            Action::EditAsGraph => (n, Key::G),
            Action::ToggleNodeEditor => (c, Key::Space),
            Action::FrameMesh => (n, Key::F),
            Action::FrameCursor => (s, Key::F),
            Action::MaterialView => (n, Key::M),
            Action::CycleChannels => (n, Key::C),
            Action::CycleMeshMaps => (s, Key::C),
            Action::TileGrid => (n, Key::U),
            Action::Wireframe => (n, Key::W),
            Action::NextStencil => (Modifiers::ALT, Key::S),
            Action::SearchLibrary => (c, Key::F),
            Action::StencilKey => (n, Key::S),
            Action::BufferKey => (n, Key::V),
        };
        KeyboardShortcut::new(m, k)
    }
}

pub fn format(s: &KeyboardShortcut) -> String {
    let mut parts = Vec::new();
    if s.modifiers.command || s.modifiers.ctrl {
        parts.push("Ctrl");
    }
    if s.modifiers.shift {
        parts.push("Shift");
    }
    if s.modifiers.alt {
        parts.push("Alt");
    }
    let key = s.logical_key.name();
    parts.push(key);
    parts.join("+")
}

pub fn parse(text: &str) -> Option<KeyboardShortcut> {
    let mut m = Modifiers::NONE;
    let mut key = None;
    for part in text.split('+') {
        match part.trim() {
            "Ctrl" | "Cmd" => m |= Modifiers::COMMAND,
            "Shift" => m |= Modifiers::SHIFT,
            "Alt" => m |= Modifiers::ALT,
            k => key = Key::from_name(k),
        }
    }
    Some(KeyboardShortcut::new(m, key?))
}

/// Current bindings: defaults overridden by the user's preferences.
#[derive(Clone, Debug)]
pub struct Bindings {
    map: BTreeMap<Action, KeyboardShortcut>,
}

impl Bindings {
    pub fn new(overrides: &BTreeMap<Action, String>) -> Self {
        let mut map: BTreeMap<Action, KeyboardShortcut> = Action::ALL.iter().map(|a| (*a, a.default_shortcut())).collect();
        for (a, s) in overrides {
            if let Some(sc) = parse(s) {
                map.insert(*a, sc);
            }
        }
        Self { map }
    }

    pub fn get(&self, a: Action) -> KeyboardShortcut {
        self.map.get(&a).copied().unwrap_or_else(|| a.default_shortcut())
    }

    pub fn key(&self, a: Action) -> Key {
        self.get(a).logical_key
    }

    /// Actions triggered this frame. Shortcuts with more modifiers are checked first, so
    /// Ctrl+Shift+Z is not mistaken for Ctrl+Z.
    /// Actions whose shortcut was pressed, among those `allow` accepts. Their keys are consumed.
    pub fn triggered(&self, i: &mut egui::InputState, allow: impl Fn(Action) -> bool) -> Vec<Action> {
        let mut order: Vec<Action> = Action::ALL
            .into_iter()
            .filter(|a| !matches!(a, Action::StencilKey | Action::BufferKey | Action::FrameCursor) && allow(*a))
            .collect();
        let count = |m: Modifiers| m.command as u8 + m.ctrl as u8 + m.shift as u8 + m.alt as u8;
        order.sort_by_key(|a| std::cmp::Reverse(count(self.get(*a).modifiers)));
        let mut out = Vec::new();
        for a in order {
            let sc = self.get(a);
            // Exact modifier match, so Shift+C and C stay distinct.
            if i.modifiers.matches_exact(sc.modifiers) && i.key_pressed(sc.logical_key) {
                i.consume_key(sc.modifiers, sc.logical_key);
                out.push(a);
            }
        }
        out
    }
}
