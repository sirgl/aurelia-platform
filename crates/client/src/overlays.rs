/// UI layers (§39 platform/structure.md): switched on/off without mutating the simulation.
/// The hex grid is the first layer: tile readability (strategy!), not decoration.

#[derive(Debug, Clone)]
pub struct Overlays {
    /// The hex grid over the terrain (like a toggle in Nation). Default: on.
    pub show_grid: bool,
}

impl Default for Overlays {
    fn default() -> Self {
        Self { show_grid: true }
    }
}

impl Overlays {
    pub fn toggle_grid(&mut self) {
        self.show_grid = !self.show_grid;
    }
}
