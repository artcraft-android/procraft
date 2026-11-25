//! A utility for plugins (interface 24): a symmetric positive definite matrix kept by its envelope
//! (a skyline), factored and solved by the app, which does it about a hundred times as fast as a
//! plugin's interpreted code. A mesh warp bends its mesh by least squares on every frame of a drag,
//! two such solves and more each time; its own code builds the matrices and the sums.
//!
//! The plugin hands over the matrix: `n` rows, each row's first column (`u32` each, at most the
//! row's own), and the rows' entries from that column to the diagonal, one row after another
//! (`f64` each). `band_factor(first, n, values) -> i32` factors it and keeps the factor, answering
//! the number it is kept by (0: it couldn't be, not positive definite or too big). `band_solve(band,
//! b, n) -> i32` solves the kept matrix for the `n` numbers at `b` (`f64` each), in place (1: done).
//! `band_drop(band)` lets one go. A copy of a plugin's code keeps at most [`MOST_KEPT`]; another
//! lets the oldest go.

/// The most factors a copy of a plugin's code keeps.
pub(crate) const MOST_KEPT: usize = 64;
/// The most rows, entries, and work (multiplications, about the sum of the squares of the rows'
/// lengths) a matrix may have: a mesh's points twice over, at its densest, with room to spare.
const MOST_ROWS: usize = 1 << 20;
const MOST_ENTRIES: usize = 32 << 20;
const MOST_WORK: u64 = 8_000_000_000;

/// A symmetric matrix kept by its envelope, lower half only; once factored, its Cholesky factor,
/// which keeps to the same envelope.
#[derive(Debug)]
pub(crate) struct Band {
    first: Vec<usize>,
    /// Where each row starts in `a` (and one past the last row's end).
    start: Vec<usize>,
    a: Vec<f64>,
}

impl Band {
    /// The matrix of rows starting at columns `first`, entries `a`; none if they don't agree, or it
    /// is more than this does.
    pub(crate) fn new(first: &[u32], a: Vec<f64>) -> Option<Band> {
        if first.len() > MOST_ROWS || a.len() > MOST_ENTRIES {
            return None;
        }
        let mut start = Vec::with_capacity(first.len() + 1);
        let (mut at, mut work) = (0_usize, 0_u64);
        for (i, &f) in first.iter().enumerate() {
            let f = f as usize;
            if f > i {
                return None;
            }
            start.push(at);
            let len = i - f + 1;
            at += len;
            work += (len as u64).pow(2);
        }
        start.push(at);
        if at != a.len() || work > MOST_WORK || !a.iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(Band { first: first.iter().map(|&f| f as usize).collect(), start, a })
    }

    pub(crate) fn n(&self) -> usize {
        self.first.len()
    }

    fn first(&self, i: usize) -> usize {
        self.first.get(i).copied().unwrap_or(i)
    }

    /// Where entry (`i`, `j`) is kept, `first(i)` ≤ `j` ≤ `i`.
    fn index(&self, i: usize, j: usize) -> usize {
        self.start.get(i).copied().unwrap_or(0) + j - self.first(i)
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        if j < self.first(i) {
            return 0.0;
        }
        self.a.get(self.index(i, j)).copied().unwrap_or(0.0)
    }

    fn set(&mut self, i: usize, j: usize, v: f64) {
        if j >= self.first(i) {
            let k = self.index(i, j);
            if let Some(x) = self.a.get_mut(k) {
                *x = v;
            }
        }
    }

    /// The sum of row `i` times row `j` (of the factor so far) over columns `from` to `to`.
    fn dot(&self, i: usize, j: usize, from: usize, to: usize) -> f64 {
        if from >= to {
            return 0.0;
        }
        let (ri, rj) = (self.index(i, from), self.index(j, from));
        match (self.a.get(ri..ri + (to - from)), self.a.get(rj..rj + (to - from))) {
            (Some(x), Some(y)) => x.iter().zip(y).map(|(a, b)| a * b).sum(),
            _ => 0.0,
        }
    }

    /// Factors it in place (L Lᵀ); false if it isn't positive definite.
    pub(crate) fn factor(&mut self) -> bool {
        for i in 0..self.n() {
            let fi = self.first(i);
            for j in fi..i {
                let from = fi.max(self.first(j));
                let s = self.at(i, j) - self.dot(i, j, from, j);
                let d = self.at(j, j);
                self.set(i, j, s / d);
            }
            let d = self.at(i, i) - self.dot(i, i, fi, i);
            if !(d > 1e-12 && d.is_finite()) {
                return false;
            }
            self.set(i, i, d.sqrt());
        }
        true
    }

    /// Solves (L Lᵀ) x = b in place, once factored.
    pub(crate) fn solve(&self, b: &mut [f64]) {
        let n = self.n().min(b.len());
        for i in 0..n {
            let fi = self.first(i);
            let s: f64 = (fi..i).map(|k| self.at(i, k) * b.get(k).copied().unwrap_or(0.0)).sum();
            if let Some(x) = b.get_mut(i) {
                *x = (*x - s) / self.at(i, i);
            }
        }
        for i in (0..n).rev() {
            let xi = b.get(i).copied().unwrap_or(0.0) / self.at(i, i);
            if let Some(x) = b.get_mut(i) {
                *x = xi;
            }
            for k in self.first(i)..i {
                let l = self.at(i, k);
                if let Some(x) = b.get_mut(k) {
                    *x -= l * xi;
                }
            }
        }
    }
}

/// The factors a copy of a plugin's code keeps, by their numbers.
#[derive(Debug, Default)]
pub(crate) struct Bands {
    kept: Vec<(i32, Band)>,
    next: i32,
}

impl Bands {
    /// `band` kept: its number. The oldest goes if there are as many as are kept.
    pub(crate) fn keep(&mut self, band: Band) -> i32 {
        if self.kept.len() >= MOST_KEPT {
            self.kept.remove(0);
        }
        self.next = self.next.checked_add(1).unwrap_or(1);
        self.kept.push((self.next, band));
        self.next
    }

    pub(crate) fn get(&self, number: i32) -> Option<&Band> {
        self.kept.iter().find(|(n, _)| *n == number).map(|(_, b)| b)
    }

    pub(crate) fn drop_one(&mut self, number: i32) {
        self.kept.retain(|(n, _)| *n != number);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_band_solves_as_a_dense_solve_would() {
        // a random symmetric positive definite band, by rows of a least-squares sum
        let (n, w) = (40_usize, 5_usize);
        let mut seed = 7u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) - 0.5
        };
        let first: Vec<u32> = (0..n).map(|i| i.saturating_sub(w) as u32).collect();
        let mut dense = vec![vec![0.0f64; n]; n];
        for i in 0..n {
            let row: Vec<(usize, f64)> =
                (i..(i + w + 1).min(n)).map(|j| (j, next() + if j == i { 2.0 } else { 0.0 })).collect();
            for &(a, x) in &row {
                for &(b, y) in &row {
                    dense[a][b] += x * y;
                }
            }
        }
        let a: Vec<f64> =
            (0..n).flat_map(|i| (first[i] as usize..=i).map(|j| dense[i][j]).collect::<Vec<_>>()).collect();
        let mut band = Band::new(&first, a).unwrap();
        let want: Vec<f64> = (0..n).map(|_| next()).collect();
        let mut b: Vec<f64> = (0..n).map(|i| (0..n).map(|j| dense[i][j] * want[j]).sum()).collect();
        assert!(band.factor());
        band.solve(&mut b);
        for (got, want) in b.iter().zip(&want) {
            assert!((got - want).abs() < 1e-8, "{got} {want}");
        }
    }

    #[test]
    fn a_band_that_does_not_hold_up_is_refused_and_few_are_kept() {
        assert!(Band::new(&[0, 2], vec![1.0, 1.0]).is_none(), "a row starting past its diagonal");
        assert!(Band::new(&[0, 0], vec![1.0, 1.0]).is_none(), "entries short");
        assert!(Band::new(&[0], vec![f64::NAN]).is_none());
        let mut not = Band::new(&[0, 0], vec![1.0, 2.0, 1.0]).unwrap();
        assert!(!not.factor(), "not positive definite");
        let mut bands = Bands::default();
        let numbers: Vec<i32> = (0..MOST_KEPT + 1).map(|_| bands.keep(Band::new(&[0], vec![4.0]).unwrap())).collect();
        assert!(bands.get(numbers[0]).is_none() && bands.get(numbers[MOST_KEPT]).is_some());
        bands.drop_one(numbers[MOST_KEPT]);
        assert!(bands.get(numbers[MOST_KEPT]).is_none());
    }
}
