//! Key → id index of a ruleset under compilation.
//!
//! Every table is sorted by key and its id is the position in that order,
//! so the index is known before any row is compiled: rows resolve their
//! references (to any table, in any direction) in one pass, by hash lookup.
//! Keys are unique across all tables (`DuplicateId` otherwise).

use std::collections::{HashMap, HashSet};

use crate::rules::compile::RulesError;
use crate::rules::content::{ContentRef, ContentTable};
use crate::rules::source::StableContentKey;

/// Stable key of every source record of one table, in source order
/// (missing ids get a positional `auto` key) plus the key order.
pub struct TableKeys {
    /// Key of source row i.
    pub keys: Vec<String>,
    /// Source row indices sorted by key: `order[id] = source row`.
    pub order: Vec<usize>,
}

impl TableKeys {
    pub fn of<'a>(ids: impl Iterator<Item = &'a Option<StableContentKey>>, table: &str) -> Self {
        let mut keys = Vec::new();
        for (i, id) in ids.enumerate() {
            match id {
                Some(key) => keys.push(key.0.clone()),
                None => keys.push(format!("core:{table}/auto{i}")),
            }
        }
        let mut order: Vec<usize> = (0..keys.len()).collect();
        order.sort_by(|a, b| keys[*a].cmp(&keys[*b]));
        Self { keys, order }
    }

    /// Key of the row with id `id`.
    pub fn key_of(&self, id: usize) -> &str {
        &self.keys[self.order[id]]
    }
}

/// Every key of the content: its table and its id there.
pub struct KeyIndex {
    ids: HashMap<String, (ContentTable, u16)>,
    unit_classes: HashSet<String>,
    building_classes: HashSet<String>,
}

/// Vocabulary namespaces that have no table: a key is valid when its slug
/// is in the list (or any slug for open vocabularies).
const UNIT_COMBAT_SLUGS: [&str; 15] = [
    "recon",
    "melee",
    "archer",
    "mounted",
    "siege",
    "gun",
    "armor",
    "naval",
    "navalmelee",
    "navalranged",
    "submarine",
    "carrier",
    "fighter",
    "bomber",
    "helicopter",
];
const DOMAIN_SLUGS: [&str; 4] = ["land", "sea", "air", "immobile"];
/// Open vocabularies: actions, routes, slots, leaders, traits, processes.
const OPEN_NAMESPACES: [&str; 6] = [
    "build",
    "route",
    "great-work-slot",
    "leader",
    "trait",
    "process",
];

impl KeyIndex {
    pub fn new() -> Self {
        Self {
            ids: HashMap::new(),
            unit_classes: HashSet::new(),
            building_classes: HashSet::new(),
        }
    }

    /// Register a table: ids in key order; a key seen in any table before
    /// is a `DuplicateId`.
    pub(crate) fn add_table(&mut self, table: ContentTable, keys: &TableKeys) -> Result<(), RulesError> {
        for (id, row) in keys.order.iter().enumerate() {
            let key = &keys.keys[*row];
            if id > u16::MAX as usize {
                return Err(RulesError::TooMany(format!("{table:?} table")));
            }
            if self.ids.insert(key.clone(), (table, id as u16)).is_some() {
                return Err(RulesError::DuplicateId(key.clone()));
            }
        }
        Ok(())
    }

    pub(crate) fn add_unit_class(&mut self, key: &str) {
        self.unit_classes.insert(key.to_string());
    }

    pub(crate) fn add_building_class(&mut self, key: &str) {
        self.building_classes.insert(key.to_string());
    }

    /// Id of `key` in `table` (UnknownReference when absent or in another).
    pub fn id(&self, table: ContentTable, key: &str) -> Result<u16, RulesError> {
        match self.ids.get(key) {
            Some((t, id)) if *t == table => Ok(*id),
            _ => Err(RulesError::UnknownReference(key.to_string())),
        }
    }

    pub fn opt_id(
        &self,
        table: ContentTable,
        key: &Option<StableContentKey>,
    ) -> Result<Option<u16>, RulesError> {
        match key {
            Some(k) => self.id(table, &k.0).map(Some),
            None => Ok(None),
        }
    }

    pub fn ids(
        &self,
        table: ContentTable,
        keys: &[StableContentKey],
    ) -> Result<Vec<u16>, RulesError> {
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            out.push(self.id(table, &key.0)?);
        }
        Ok(out)
    }

    /// Resolve any reference of an inert effect: a table row, a declared
    /// unit or building class, or a vocabulary key.
    pub fn any(&self, key: &str) -> Result<ContentRef, RulesError> {
        if let Some((table, index)) = self.ids.get(key) {
            return Ok(ContentRef {
                table: *table,
                index: *index,
                key: key.to_string(),
            });
        }
        let table = self.class_or_vocabulary(key)?;
        Ok(ContentRef {
            table,
            index: 0,
            key: key.to_string(),
        })
    }

    fn class_or_vocabulary(&self, key: &str) -> Result<ContentTable, RulesError> {
        let unknown = || RulesError::UnknownReference(key.to_string());
        let rest = key.strip_prefix("core:").ok_or_else(unknown)?;
        let (namespace, slug) = rest.split_once('/').ok_or_else(unknown)?;
        let known = match namespace {
            "unit-class" if self.unit_classes.contains(key) => ContentTable::UnitClass,
            "building-class" if self.building_classes.contains(key) => ContentTable::BuildingClass,
            "unit-combat" if UNIT_COMBAT_SLUGS.contains(&slug) => ContentTable::Vocabulary,
            "domain" if DOMAIN_SLUGS.contains(&slug) => ContentTable::Vocabulary,
            ns if OPEN_NAMESPACES.contains(&ns) && !slug.is_empty() => ContentTable::Vocabulary,
            _ => return Err(unknown()),
        };
        Ok(known)
    }
}
