use crate::ids::PlayerId;

/// War and open borders, the diplomacy the kernel knows (movement, combat
/// and navigation read it); treaties and deals are the rules module's.
/// Dense matrices for determinism.
#[derive(Debug, Clone, Default)]
pub struct DiplomacyState {
    /// row-major `players x players`: 0 peace, 1 war (symmetric).
    matrix: Vec<u8>,
    /// Open borders (DIRECTED): open[a*P+b] = a lets b in.
    /// In peacetime foreign territory is closed without a grant (§A3).
    open: Vec<u8>,
    players: usize,
}

impl DiplomacyState {
    pub fn new(players: usize) -> Self {
        Self {
            matrix: vec![0; players * players],
            open: vec![0; players * players],
            players,
        }
    }

    pub fn at_war(&self, a: PlayerId, b: PlayerId) -> bool {
        let (a, b) = (a.0 as usize, b.0 as usize);
        if a >= self.players || b >= self.players {
            return false;
        }
        self.matrix[a * self.players + b] == 1
    }

    /// Raw row-major matrix (stable order), for hash/save.
    pub(crate) fn matrix(&self) -> &[u8] {
        &self.matrix
    }

    /// Open-borders matrix (the same layout), for hash/save.
    pub(crate) fn open_matrix(&self) -> &[u8] {
        &self.open
    }

    /// Restore from the save schema (lengths are checked in decode before the call).
    pub(crate) fn from_parts(players: usize, matrix: Vec<u8>, open: Vec<u8>) -> Self {
        debug_assert_eq!(matrix.len(), players * players);
        debug_assert_eq!(open.len(), players * players);
        Self {
            matrix,
            open,
            players,
        }
    }

    pub fn set_war(&mut self, a: PlayerId, b: PlayerId, war: bool) {
        let (a, b) = (a.0 as usize, b.0 as usize);
        if a < self.players && b < self.players {
            let v = u8::from(war);
            self.matrix[a * self.players + b] = v;
            self.matrix[b * self.players + a] = v;
        }
    }

    /// Whether host lets guest in (directed; oneself always).
    pub fn grants_entry(&self, host: PlayerId, guest: PlayerId) -> bool {
        if host == guest {
            return true;
        }
        let (h, g) = (host.0 as usize, guest.0 as usize);
        if h >= self.players || g >= self.players {
            return false;
        }
        self.open[h * self.players + g] == 1
    }

    /// Grant/revoke a grant (directed). War does not touch the grant
    /// (returning to peace restores the previous mode, as in Nation).
    pub fn set_open(&mut self, host: PlayerId, guest: PlayerId, open: bool) {
        let (h, g) = (host.0 as usize, guest.0 as usize);
        if h < self.players && g < self.players {
            self.open[h * self.players + g] = u8::from(open);
        }
    }
}
