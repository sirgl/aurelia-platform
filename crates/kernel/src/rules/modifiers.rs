//! Evaluation order of numeric modifiers — part of the gameplay spec
//! (platform/project.md §21, platform/structure.md "Order of effect evaluation"):
//!
//! ```text
//! 1. base         value computed by the system (tiles, citizens, content)
//! 2. flat         + Σ AddYield / ModifyMovement amounts
//! 3. additive %   × (100 + Σ percents) / 100   (AddYieldPercent, ModifyCost,
//!                                               ModifyCombat, ModifyGrowth)
//! 4. multiplicative × Π (percent / 100)          (MultiplyYield, Unique only)
//! 5. caps         clamp to the consumer's [min, max]
//! 6. rounding     toward zero, once, at the end
//! ```
//!
//! Stages 3–5 run in hundredths (`value × 100`) so the only rounding is
//! stage 6. The multiplicative product is kept exact (numerator Π percent,
//! denominator 100^count): the result does not depend on the order in which
//! sources were visited.

/// Accumulated modifiers of one value, one entry per stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StageTotals {
    pub flat: i32,
    pub percent: i32,
    /// Number of multiplicative factors, at most `MAX_MULTIPLIERS`.
    multiplier_count: u8,
    /// Π of the factors in percent (100 = ×1); exact in i128 for 8 factors
    /// up to 10 000 % each.
    multiplier_product: i128,
}

/// Distinct multiplicative modifiers on one value. They are Unique-only and
/// the compiler limits the ruleset to this many per yield kind, so one
/// target can never collect more.
pub const MAX_MULTIPLIERS: usize = 8;

impl Default for StageTotals {
    fn default() -> Self {
        Self {
            flat: 0,
            percent: 0,
            multiplier_count: 0,
            multiplier_product: 1,
        }
    }
}

/// Bounds of stage 5, set by the consumer of the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    pub min: Option<i32>,
    pub max: Option<i32>,
}

impl Caps {
    pub const NONE: Caps = Caps {
        min: None,
        max: None,
    };

    pub fn at_least(min: i32) -> Caps {
        Caps {
            min: Some(min),
            max: None,
        }
    }
}

impl StageTotals {
    pub(crate) fn add_flat(&mut self, amount: i32) {
        self.flat = self.flat.saturating_add(amount);
    }

    pub(crate) fn add_percent(&mut self, percent: i32) {
        self.percent = self.percent.saturating_add(percent);
    }

    /// The compiler bound (`MAX_MULTIPLIERS` per yield kind) makes overflow
    /// impossible for compiled content.
    pub(crate) fn add_multiplier(&mut self, percent: i32) {
        let n = self.multiplier_count as usize;
        assert!(
            n < MAX_MULTIPLIERS,
            "compiler bounds multiplicative modifiers"
        );
        self.multiplier_product *= i128::from(percent);
        self.multiplier_count += 1;
    }

    /// Add every stage of `other` (flat and percent sums, multipliers
    /// multiplied). Used to fold a legacy channel into another one.
    pub fn merge(&mut self, other: &StageTotals) {
        self.add_flat(other.flat);
        self.add_percent(other.percent);
        let count = self.multiplier_count as usize + other.multiplier_count as usize;
        assert!(count <= MAX_MULTIPLIERS, "compiler bounds multiplicative modifiers");
        self.multiplier_product *= other.multiplier_product;
        self.multiplier_count = count as u8;
    }

    /// Stages 2–6 over `base` (see the module docs).
    pub fn evaluate(&self, base: i32, caps: Caps) -> i32 {
        let flat = i64::from(base) + i64::from(self.flat);
        let hundredths = flat * (100 + i64::from(self.percent));
        let multiplied = self.multiply(hundredths);
        let capped = apply_caps(multiplied, caps);
        let rounded = capped / 100;
        rounded.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
    }

    /// Stage 4, exact: × Π percent / 100^count (truncated toward zero).
    fn multiply(&self, hundredths: i64) -> i64 {
        if self.multiplier_count == 0 {
            return hundredths;
        }
        let denominator = 100i128.pow(u32::from(self.multiplier_count));
        let value = i128::from(hundredths) * self.multiplier_product / denominator;
        value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    }
}

fn apply_caps(hundredths: i64, caps: Caps) -> i64 {
    let mut value = hundredths;
    if let Some(min) = caps.min {
        value = value.max(i64::from(min) * 100);
    }
    if let Some(max) = caps.max {
        value = value.min(i64::from(max) * 100);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn neutral_totals_return_base() {
        let t = StageTotals::default();
        for base in [-7, 0, 1, 13, 1000] {
            assert_eq!(t.evaluate(base, Caps::NONE), base);
        }
    }

    #[test]
    fn stages_run_in_spec_order() {
        // (10 + 5) × (100 + 50)% = 22.5 → × 200% = 45 → caps none → 45.
        let mut t = StageTotals::default();
        t.add_flat(5);
        t.add_percent(30);
        t.add_percent(20);
        t.add_multiplier(200);
        assert_eq!(t.evaluate(10, Caps::NONE), 45);
        // Rounding happens once: 15 × 115% = 17.25 → 17 (not 15 + 2.25).
        let mut p = StageTotals::default();
        p.add_flat(5);
        p.add_percent(15);
        assert_eq!(p.evaluate(10, Caps::NONE), 17);
    }

    #[test]
    fn non_negative_values_match_v1_formula() {
        // v1 rounded the percent part alone (v + v·p/100); with v, p ≥ 0
        // that equals one rounding at the end — all current content.
        for v in 0..80 {
            for p in [0, 10, 15, 33, 120] {
                let mut t = StageTotals::default();
                t.add_percent(p);
                assert_eq!(t.evaluate(v, Caps::NONE), v + v * p / 100, "v={v} p={p}");
            }
        }
    }

    #[test]
    fn rounding_is_once_and_toward_zero() {
        // 15 × 90% = 13.5 → 13 (v1 would give 15 + trunc(−1.5) = 14).
        let mut t = StageTotals::default();
        t.add_percent(-10);
        assert_eq!(t.evaluate(15, Caps::NONE), 13);
        // Negative values round toward zero too: −39 × 50% = −19.5 → −19.
        let mut n = StageTotals::default();
        n.add_percent(-50);
        assert_eq!(n.evaluate(-39, Caps::NONE), -19);
    }

    #[test]
    fn multipliers_do_not_depend_on_order() {
        let mut a = StageTotals::default();
        let mut b = StageTotals::default();
        for m in [133, 150, 75] {
            a.add_multiplier(m);
        }
        for m in [75, 133, 150] {
            b.add_multiplier(m);
        }
        assert_eq!(a.evaluate(97, Caps::NONE), b.evaluate(97, Caps::NONE));
    }

    #[test]
    fn caps_apply_before_rounding() {
        let mut t = StageTotals::default();
        t.add_percent(-90);
        assert_eq!(t.evaluate(5, Caps::at_least(1)), 1);
        let max = Caps {
            min: None,
            max: Some(10),
        };
        assert_eq!(t.evaluate(500, max), 10);
    }
}
