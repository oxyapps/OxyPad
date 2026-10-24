//! Random passwords with a number of each kind of character, as MacPad++
//! makes them.
//!
//! Every kind with a count above zero is used; the rest of the length is
//! drawn from all of them, then the characters are shuffled. Randomness comes
//! from the system's cryptographically secure generator.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

pub const UPPERCASE: &str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";
pub const LOWERCASE: &str = "abcdefghijklmnopqrstuvwxyz";
pub const DIGITS: &str = "0123456789";
pub const SYMBOLS: &str = "!#$%&()*+,-./:;<=>?@[]^_{|}~";
/// 0, O and o; 1, I, l and |; and the pairs S 5, B 8, Z 2, G 6.
pub const AMBIGUOUS: &str = "0Oo1Il|S5B8Z2G6";

/// How the length is shared out by default: 16 characters are 5 uppercase,
/// 5 lowercase, 4 numbers and 2 symbols.
const WEIGHTS: [usize; 4] = [5, 5, 4, 2];

/// Missing fields take their default, so it can live in the preferences.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PasswordGenerator {
    pub length: usize,
    pub uppercase: usize,
    pub lowercase: usize,
    pub digits: usize,
    pub symbols: usize,
    /// Leaves out characters that are easy to mistake for one another.
    pub exclude_ambiguous: bool,
}

impl Default for PasswordGenerator {
    fn default() -> Self {
        Self { length: 16, uppercase: 5, lowercase: 5, digits: 4, symbols: 2, exclude_ambiguous: false }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordProblem {
    /// No kind of character has a count above zero.
    NoKinds,
    /// The counts add up to more than the length.
    CountsExceedLength,
}

impl PasswordGenerator {
    pub const LENGTHS: RangeInclusive<usize> = 4..=128;

    /// The counts of each kind: uppercase, lowercase, numbers, symbols.
    pub fn counts(&self) -> [usize; 4] {
        [self.uppercase, self.lowercase, self.digits, self.symbols]
    }

    fn set_counts(&mut self, counts: [usize; 4]) {
        [self.uppercase, self.lowercase, self.digits, self.symbols] = counts;
    }

    /// A new length, shared out again among the kinds in use.
    pub fn set_length(&mut self, length: usize) {
        self.length = length.clamp(*Self::LENGTHS.start(), *Self::LENGTHS.end());
        let mut enabled = self.counts().map(|count| count > 0);
        if !enabled.contains(&true) {
            enabled = [true; 4];
        }
        self.set_counts(distribute(self.length, enabled));
    }

    /// One kind set by hand: the length stays, and the other kinds in use
    /// share what is left, keeping at least one each. 0 leaves a kind out.
    pub fn set_count(&mut self, index: usize, value: usize) {
        let mut others = self.counts().map(|count| count > 0);
        others[index] = false;
        let other_count = others.iter().filter(|&&used| used).count();
        // The only kind in use has nowhere to give characters to.
        if other_count == 0 {
            let mut counts = [0; 4];
            counts[index] = self.length;
            self.set_counts(counts);
            return;
        }
        let kept = value.min(self.length.saturating_sub(other_count));
        let mut counts = distribute(self.length - kept, others);
        counts[index] = kept;
        self.set_counts(counts);
    }

    /// Makes the counts add up to the length again, for settings saved by
    /// hand or by an older version.
    pub fn normalized(mut self) -> Self {
        if self.length != self.length.clamp(*Self::LENGTHS.start(), *Self::LENGTHS.end())
            || self.counts().iter().sum::<usize>() != self.length
        {
            self.set_length(self.length);
        }
        self
    }

    pub fn problem(&self) -> Option<PasswordProblem> {
        let counts = self.counts();
        if counts.iter().all(|&count| count == 0) {
            Some(PasswordProblem::NoKinds)
        } else if counts.iter().sum::<usize>() > self.length {
            Some(PasswordProblem::CountsExceedLength)
        } else {
            None
        }
    }

    /// The characters of each kind in use, paired with its count.
    fn kinds(&self) -> Vec<(Vec<char>, usize)> {
        [UPPERCASE, LOWERCASE, DIGITS, SYMBOLS]
            .into_iter()
            .zip(self.counts())
            .filter(|&(_, count)| count > 0)
            .map(|(set, count)| {
                let characters = set.chars().filter(|ch| !self.exclude_ambiguous || !AMBIGUOUS.contains(*ch));
                (characters.collect(), count)
            })
            .collect()
    }

    /// A new password, or `None` when the settings have a problem or the
    /// system has no randomness to give.
    pub fn generate(&self) -> Option<String> {
        if self.problem().is_some() {
            return None;
        }
        let kinds = self.kinds();
        let mut characters = Vec::with_capacity(self.length);
        for (set, count) in &kinds {
            for _ in 0..*count {
                characters.push(set[random_below(set.len())?]);
            }
        }
        let pool: Vec<char> = kinds.iter().flat_map(|(set, _)| set.iter().copied()).collect();
        while characters.len() < self.length {
            characters.push(pool[random_below(pool.len())?]);
        }
        // Fisher–Yates.
        for ix in (1..characters.len()).rev() {
            characters.swap(ix, random_below(ix + 1)?);
        }
        Some(characters.into_iter().collect())
    }
}

/// Splits `total` among the kinds marked in `enabled`, in proportion to
/// `WEIGHTS`, rounding so the counts add up exactly (largest remainder).
/// Every enabled kind gets at least one character while there are enough.
fn distribute(total: usize, enabled: [bool; 4]) -> [usize; 4] {
    let weights: [usize; 4] = std::array::from_fn(|ix| if enabled[ix] { WEIGHTS[ix] } else { 0 });
    let weight_sum: usize = weights.iter().sum();
    if weight_sum == 0 || total == 0 {
        return [0; 4];
    }
    let shares = weights.map(|weight| (total * weight) as f64 / weight_sum as f64);
    let mut counts = shares.map(|share| share.floor() as usize);
    let remainder = |ix: usize, counts: &[usize; 4]| shares[ix] - counts[ix] as f64;
    let mut by_remainder: Vec<usize> = (0..4).filter(|&ix| weights[ix] > 0).collect();
    // Stable, so equal remainders keep their order.
    by_remainder.sort_by(|&a, &b| remainder(b, &counts).total_cmp(&remainder(a, &counts)));
    let missing = total - counts.iter().sum::<usize>();
    for ix in by_remainder.into_iter().take(missing) {
        counts[ix] += 1;
    }
    // A small total can round an enabled kind down to nothing; take one from
    // the largest (the first of equals) instead.
    for ix in 0..4 {
        if enabled[ix] && counts[ix] == 0 {
            let largest = (0..4).fold(0, |best, each| if counts[each] > counts[best] { each } else { best });
            if counts[largest] <= 1 {
                break;
            }
            counts[largest] -= 1;
            counts[ix] += 1;
        }
    }
    counts
}

/// A uniformly random number below `bound`, from the system's secure
/// generator, rejecting the values that would favor small numbers.
pub(crate) fn random_below(bound: usize) -> Option<usize> {
    let bound = u32::try_from(bound).ok().filter(|&bound| bound > 0)?;
    let zone = u32::MAX - u32::MAX % bound;
    loop {
        let value = getrandom::u32().ok()?;
        if value < zone {
            return Some((value % bound) as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn generator(length: usize, counts: [usize; 4]) -> PasswordGenerator {
        let mut generator = PasswordGenerator { length, ..Default::default() };
        generator.set_counts(counts);
        generator
    }

    fn count(password: &str, set: &str) -> usize {
        password.chars().filter(|ch| set.contains(*ch)).count()
    }

    #[test]
    fn length_and_counts() {
        let generator = generator(16, [3, 2, 4, 5]);
        for _ in 0..200 {
            let password = generator.generate().unwrap();
            assert_eq!(password.chars().count(), 16);
            assert!(count(&password, UPPERCASE) >= 3);
            assert!(count(&password, LOWERCASE) >= 2);
            assert!(count(&password, DIGITS) >= 4);
            assert!(count(&password, SYMBOLS) >= 5);
        }
    }

    #[test]
    fn zero_leaves_a_kind_out() {
        let generator = generator(64, [0, 1, 1, 0]);
        for _ in 0..50 {
            let password = generator.generate().unwrap();
            assert_eq!(count(&password, UPPERCASE), 0);
            assert_eq!(count(&password, SYMBOLS), 0);
        }
    }

    #[test]
    fn exclude_ambiguous() {
        let mut generator = PasswordGenerator { exclude_ambiguous: true, ..Default::default() };
        generator.set_length(128);
        for _ in 0..50 {
            assert_eq!(count(&generator.generate().unwrap(), AMBIGUOUS), 0);
        }
    }

    #[test]
    fn problems() {
        assert_eq!(generator(16, [0, 0, 0, 0]).problem(), Some(PasswordProblem::NoKinds));
        assert_eq!(generator(4, [2, 2, 1, 0]).problem(), Some(PasswordProblem::CountsExceedLength));
        // Counts that fill the length exactly are fine.
        assert_eq!(generator(4, [2, 2, 0, 0]).generate().map(|p| p.len()), Some(4));
    }

    #[test]
    fn defaults_are_five_five_four_two() {
        let generator = PasswordGenerator::default();
        assert_eq!(generator.length, 16);
        assert_eq!(generator.counts(), [5, 5, 4, 2]);
    }

    #[test]
    fn length_is_shared_out_in_proportion() {
        let mut generator = PasswordGenerator::default();
        generator.set_length(20);
        assert_eq!(generator.counts(), [6, 6, 5, 3]);
        generator.set_length(32);
        assert_eq!(generator.counts(), [10, 10, 8, 4]);
        generator.set_length(4);
        assert_eq!(generator.counts(), [1, 1, 1, 1]);
        for length in PasswordGenerator::LENGTHS {
            generator.set_length(length);
            assert_eq!(generator.counts().iter().sum::<usize>(), length);
        }
    }

    #[test]
    fn kinds_left_out_stay_out() {
        let mut generator = PasswordGenerator::default();
        generator.set_count(3, 0);
        assert_eq!(generator.counts(), [6, 6, 4, 0]);
        generator.set_length(28);
        assert_eq!(generator.counts()[3], 0);
        assert_eq!(generator.counts().iter().sum::<usize>(), 28);
    }

    #[test]
    fn setting_a_kind_adjusts_the_others() {
        let mut generator = PasswordGenerator::default();
        generator.set_count(2, 8);
        assert_eq!(generator.length, 16);
        assert_eq!(generator.counts()[2], 8);
        assert_eq!(generator.counts().iter().sum::<usize>(), 16);
        // Others keep at least one each.
        generator.set_count(0, 16);
        assert_eq!(generator.counts(), [13, 1, 1, 1]);
        // A kind left out comes back by taking from the others.
        generator.set_count(3, 0);
        generator.set_count(3, 3);
        assert_eq!(generator.counts()[3], 3);
        assert_eq!(generator.counts().iter().sum::<usize>(), 16);
    }

    #[test]
    fn the_only_kind_keeps_the_whole_length() {
        let mut generator = generator(10, [0, 10, 0, 0]);
        generator.set_count(1, 4);
        assert_eq!(generator.counts(), [0, 10, 0, 0]);
    }

    #[test]
    fn normalized_shares_out_odd_settings() {
        let generator = generator(200, [1, 1, 1, 1]).normalized();
        assert_eq!(generator.length, 128);
        assert_eq!(generator.counts().iter().sum::<usize>(), 128);
        assert_eq!(PasswordGenerator::default().normalized(), PasswordGenerator::default());
    }
}
