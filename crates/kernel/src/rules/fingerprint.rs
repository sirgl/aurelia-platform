//! Canonical content fingerprint of a CompiledRuleset (save header and
//! replay guard, platform/project.md §26).
//!
//! Every table is written in stable key order, every field of every row
//! (effects and inert effects included). Rows derive `Hash`, so a new field
//! is part of the fingerprint the moment it exists. The hasher is FNV-1a 64
//! with platform-independent integer encoding (`usize`/`isize` and enum
//! discriminants as 64-bit little endian): identical on every platform,
//! wasm included. Derived data (`effect_index`, `terrain_kinds`) is left
//! out: it is a function of the rows and roles.
//!
//! Schema additions that existing saves predate are hashed in their own
//! section, so the fingerprint those saves carry stays reproducible
//! (`Edition`): the snapshot loader accepts them, replay stays strict.

use crate::module::RulesModule;
use std::hash::{Hash, Hasher};

use crate::rules::compiled::CompiledRuleset;
use crate::rules::tables::ModuleTables;
use crate::world::module::ModuleWorld;

/// FNV-1a 64 with fixed-width integers.
pub struct Fingerprint(u64);

impl Fingerprint {
    fn new() -> Self {
        Fingerprint(0xcbf2_9ce4_8422_2325)
    }
}

impl Hasher for Fingerprint {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 ^= u64::from(*b);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn write_usize(&mut self, i: usize) {
        self.write(&(i as u64).to_le_bytes());
    }

    fn write_isize(&mut self, i: isize) {
        self.write(&(i as i64).to_le_bytes());
    }

    fn write_u16(&mut self, i: u16) {
        self.write(&i.to_le_bytes());
    }

    fn write_u32(&mut self, i: u32) {
        self.write(&i.to_le_bytes());
    }

    fn write_u64(&mut self, i: u64) {
        self.write(&i.to_le_bytes());
    }

    fn write_i16(&mut self, i: i16) {
        self.write(&i.to_le_bytes());
    }

    fn write_i32(&mut self, i: i32) {
        self.write(&i.to_le_bytes());
    }

    fn write_i64(&mut self, i: i64) {
        self.write(&i.to_le_bytes());
    }
}

/// The fingerprints a save may carry, oldest first: the current one, and
/// the earlier ones the snapshot loader migrates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Edition {
    BeforeCityNames,
    BeforeModuleId,
    /// Before role bindings (`rules::roles`).
    BeforeRoles,
    Current,
}

impl<M: RulesModule> CompiledRuleset<M> {
    /// Canonical content hash for the save header / replay guard: the
    /// rules module's id and version, and any change of any rule (a number,
    /// a reference, an effect, a role binding) change it.
    pub fn hash(&self) -> u64 {
        self.fingerprint(Edition::Current)
    }

    /// The exact fingerprint before role bindings (`rules::roles`) were
    /// added. Only the explicitly compatible snapshot loader may use this;
    /// replay stays strict.
    pub fn hash_before_roles(&self) -> u64 {
        self.fingerprint(Edition::BeforeRoles)
    }

    /// The exact fingerprint before it named the rules module. Only the
    /// explicitly compatible snapshot loader may use this; replay stays strict.
    pub fn hash_before_module_id(&self) -> u64 {
        self.fingerprint(Edition::BeforeModuleId)
    }

    /// The exact fingerprint before city-name pools were added (and before
    /// the module id). Only the snapshot loader may use this.
    pub fn hash_before_city_names(&self) -> u64 {
        self.fingerprint(Edition::BeforeCityNames)
    }

    /// The kernel's tables in their fixed order; after each, the module's
    /// tables that followed it before the split (`ModuleTables::hash_after`).
    fn fingerprint(&self, edition: Edition) -> u64 {
        let mut f = Fingerprint::new();
        if edition >= Edition::BeforeRoles {
            <M::World as ModuleWorld<M>>::ID.hash(&mut f);
        }
        self.version.hash(&mut f);
        // The nation table keeps its historical name "civs" in the fingerprint: the name is hashed, so renaming it
        // would make every earlier save report "made with other rules".
        self.each_kernel_table(|name, hash, _| {
            if name == "speeds" {
                // Speeds came after every earlier edition: only the current one holds them.
                if edition == Edition::Current {
                    hash(&mut f);
                }
                return;
            }
            if name == "civs" && edition < Edition::BeforeModuleId {
                legacy_nations(&mut f, &self.nations);
            } else {
                hash(&mut f);
            }
            self.module.hash_after(name, &mut f);
        });
        if edition == Edition::Current {
            "roles".hash(&mut f);
            self.roles.hash(&mut f);
        }
        f.finish()
    }

    /// Each kernel table in the fingerprint's order: its name, its hash, and its
    /// shape (the row count and the keys in row order: what the rows' numbers mean,
    /// whatever the rows are called).
    fn each_kernel_table(&self, mut visit: impl FnMut(&'static str, &dyn Fn(&mut Fingerprint), &dyn Fn(&mut Fingerprint))) {
        visit("units", &|f| f.table("units", &self.units, |d| &d.key), &|f| f.shape(&self.units, |d| &d.key));
        visit("buildings", &|f| f.table("buildings", &self.buildings, |d| &d.key), &|f| f.shape(&self.buildings, |d| &d.key));
        visit("technologies", &|f| f.table("technologies", &self.technologies, |d| &d.key), &|f| f.shape(&self.technologies, |d| &d.key));
        visit("improvements", &|f| f.table("improvements", &self.improvements, |d| &d.key), &|f| f.shape(&self.improvements, |d| &d.key));
        visit("features", &|f| f.table("features", &self.features, |d| &d.key), &|f| f.shape(&self.features, |d| &d.key));
        visit("resources", &|f| f.table("resources", &self.resources, |d| &d.key), &|f| f.shape(&self.resources, |d| &d.key));
        visit("civs", &|f| f.table("civs", &self.nations, |d| &d.key), &|f| f.shape(&self.nations, |d| &d.key));
        visit("promotions", &|f| f.table("promotions", &self.promotions, |d| &d.key), &|f| f.shape(&self.promotions, |d| &d.key));
        visit("eras", &|f| f.table("eras", &self.eras, |d| &d.key), &|f| f.shape(&self.eras, |d| &d.key));
        visit("terrains", &|f| f.table("terrains", &self.terrains, |d| &d.key), &|f| f.shape(&self.terrains, |d| &d.key));
        visit("natural_wonders", &|f| f.table("natural_wonders", &self.natural_wonders, |d| &d.key), &|f| f.shape(&self.natural_wonders, |d| &d.key));
        visit("specialists", &|f| f.table("specialists", &self.specialists, |d| &d.key), &|f| f.shape(&self.specialists, |d| &d.key));
        visit("handicaps", &|f| f.table("handicaps", &self.handicaps, |d| &d.key), &|f| f.shape(&self.handicaps, |d| &d.key));
        // Rules without speeds (the frozen fixtures, the sandbox) keep the fingerprint they had before speeds.
        if !self.speeds.is_empty() {
            visit("speeds", &|f| f.table("speeds", &self.speeds, |d| &d.key), &|f| f.shape(&self.speeds, |d| &d.key));
        }
    }
}

/// The fingerprint of each part of a ruleset, in the fingerprint's order:
/// the module's id, the version, each kernel table, the module's tables
/// after it (when it hashes any) and the role bindings. A save that keeps
/// it lets a load on other rules name what changed (`decode::RulesDrift`).
///
/// Each kernel table is followed by the part "rows of <table>": the shape of
/// the table (its row count and its keys in row order), which the names and
/// the numbers in the rows do not enter. A world points into a table by row
/// number, so only a change of the shape can move what a saved number means;
/// a save older than this part has none ([`RulesDigest::shifted_tables`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RulesDigest(pub Vec<(String, u64)>);

/// The name prefix of the shape part of a kernel table in a [`RulesDigest`].
const SHAPE: &str = "rows of ";

impl RulesDigest {
    fn saved(&self, name: &str) -> Option<u64> {
        self.0.iter().find(|(n, _)| n == name).map(|(_, f)| *f)
    }

    /// The parts of `current` whose fingerprint differs from this digest's
    /// or that this digest lacks, then the parts `current` no longer has.
    /// The shape parts are not listed: they say how a change reads, not what
    /// changed ([`RulesDigest::shifted_tables`]).
    pub fn changed_parts(&self, current: &RulesDigest) -> Vec<String> {
        let shown = |name: &String| !name.starts_with(SHAPE);
        let changed = current.0.iter().filter(|(name, f)| shown(name) && self.saved(name) != Some(*f)).map(|(name, _)| name.clone());
        let gone = self.0.iter().filter(|(name, _)| shown(name) && !current.0.iter().any(|(n, _)| n == name)).map(|(name, _)| name.clone());
        changed.chain(gone).collect()
    }

    /// The kernel tables of `current` whose rows may have moved since this
    /// digest: the keys or their order changed, so a row number saved before
    /// may mean another row now. A pure rename (the names and the numbers in
    /// the rows) is not one. A digest without the shape of a table (a save of
    /// before it was kept) cannot tell, so the table counts as shifted when
    /// anything in it changed.
    pub fn shifted_tables(&self, current: &RulesDigest) -> Vec<String> {
        current
            .0
            .iter()
            .filter_map(|(name, shape)| {
                let table = name.strip_prefix(SHAPE)?;
                let moved = match self.saved(name) {
                    Some(saved) => saved != *shape,
                    None => current.saved(table).is_some_and(|now| self.saved(table) != Some(now)),
                };
                moved.then(|| table.to_owned())
            })
            .collect()
    }
}

impl<M: RulesModule> CompiledRuleset<M> {
    /// Each part's fingerprint (see [`RulesDigest`]); a part that hashes
    /// nothing (a module without tables after a kernel table) is left out.
    pub fn digest(&self) -> RulesDigest {
        let mut parts = Vec::new();
        let mut part = |name: String, hash: &dyn Fn(&mut Fingerprint)| {
            let mut f = Fingerprint::new();
            hash(&mut f);
            if f.finish() != Fingerprint::new().finish() {
                parts.push((name, f.finish()));
            }
        };
        let module = <M::World as ModuleWorld<M>>::ID;
        part("module".into(), &|f| module.hash(f));
        part("version".into(), &|f| self.version.hash(f));
        self.each_kernel_table(|name, hash, shape| {
            part(name.into(), hash);
            part(format!("{SHAPE}{name}"), shape);
            part(format!("{} tables after {name}", module.name), &|f| self.module.hash_after(name, f));
        });
        part("roles".into(), &|f| self.roles.hash(f));
        RulesDigest(parts)
    }
}

/// The nations before city names: every field but the names
/// (`NationDefinition::hash_fields` destructures the row exhaustively, so a
/// schema addition revisits this edition).
fn legacy_nations<M: RulesModule>(f: &mut Fingerprint, rows: &[crate::rules::compiled::NationDefinition<M>]) {
    "civs".hash(f);
    rows.len().hash(f);
    let mut order: Vec<_> = rows.iter().collect();
    order.sort_by(|a, b| a.key.cmp(&b.key));
    for row in order {
        row.hash_fields(f);
    }
}

impl Fingerprint {
    /// A table's shape: its size and its keys in row order, nothing else.
    pub fn shape<T>(&mut self, rows: &[T], key: fn(&T) -> &String) {
        rows.len().hash(self);
        for row in rows {
            key(row).hash(self);
        }
    }

    /// A table: its name, its size, its rows in key order (the compiler
    /// sorts, this does not rely on it).
    pub fn table<T: Hash>(&mut self, name: &str, rows: &[T], key: fn(&T) -> &String) {
        name.hash(self);
        rows.len().hash(self);
        let mut order: Vec<&T> = rows.iter().collect();
        order.sort_by(|a, b| key(a).cmp(key(b)));
        for row in order {
            row.hash(self);
        }
    }
}


#[cfg(test)]
mod tests {
    use super::RulesDigest;

    fn digest(parts: &[(&str, u64)]) -> RulesDigest {
        RulesDigest(parts.iter().map(|&(name, f)| (name.to_owned(), f)).collect())
    }

    #[test]
    fn changed_parts_are_the_differing_the_new_and_the_gone() {
        let saved = digest(&[("module", 1), ("units", 2), ("eras", 3)]);
        assert!(saved.changed_parts(&saved).is_empty());
        let current = digest(&[("module", 1), ("units", 9), ("roles", 4)]);
        assert_eq!(saved.changed_parts(&current), vec!["units", "roles", "eras"]);
    }

    #[test]
    fn a_rename_changes_a_table_but_does_not_shift_its_rows() {
        let saved = digest(&[("units", 2), ("rows of units", 7), ("eras", 3), ("rows of eras", 8)]);
        // The names in `units` changed, its keys did not.
        let renamed = digest(&[("units", 9), ("rows of units", 7), ("eras", 3), ("rows of eras", 8)]);
        assert_eq!(saved.changed_parts(&renamed), vec!["units"], "the shape is not a part the player reads");
        assert!(saved.shifted_tables(&renamed).is_empty());
        // The keys of `eras` changed: its rows may have moved.
        let moved = digest(&[("units", 2), ("rows of units", 7), ("eras", 4), ("rows of eras", 9)]);
        assert_eq!(saved.shifted_tables(&moved), vec!["eras"]);
    }

    #[test]
    fn a_digest_without_shapes_counts_a_changed_table_as_shifted() {
        let before_shapes = digest(&[("units", 2), ("eras", 3)]);
        let current = digest(&[("units", 9), ("rows of units", 7), ("eras", 3), ("rows of eras", 8)]);
        assert_eq!(before_shapes.changed_parts(&current), vec!["units"]);
        assert_eq!(before_shapes.shifted_tables(&current), vec!["units"], "it cannot tell, so it warns; an unchanged table does not");
    }
}
