//! 3×3 block compressed sparse row matrices.

/// Symmetric-pattern block CSR matrix with 3×3 blocks (row-major inside a block).
#[derive(Clone, Debug, Default)]
pub struct Bsr {
    pub n: usize,
    pub row_ptr: Vec<u32>,
    pub cols: Vec<u32>,
    pub vals: Vec<[f64; 9]>,
}

impl Bsr {
    /// Pattern from per-row neighbour lists (each must include the diagonal).
    pub fn from_adjacency(mut adj: Vec<Vec<u32>>) -> Bsr {
        let n = adj.len();
        let mut row_ptr = Vec::with_capacity(n + 1);
        row_ptr.push(0u32);
        let mut cols = Vec::new();
        for row in adj.iter_mut() {
            row.sort_unstable();
            row.dedup();
            cols.extend_from_slice(row);
            row_ptr.push(cols.len() as u32);
        }
        let nnz = cols.len();
        Bsr {
            n,
            row_ptr,
            cols,
            vals: vec![[0.0; 9]; nnz],
        }
    }

    /// Pattern of the node adjacency of a set of elements.
    pub fn from_elements<const K: usize>(n: usize, elems: impl Iterator<Item = [u32; K]>) -> Bsr {
        let mut adj: Vec<Vec<u32>> = (0..n as u32).map(|i| vec![i]).collect();
        for e in elems {
            for &a in &e {
                let row = &mut adj[a as usize];
                row.extend_from_slice(&e);
            }
        }
        Bsr::from_adjacency(adj)
    }

    #[inline]
    pub fn find(&self, i: usize, j: u32) -> usize {
        let (s, e) = (self.row_ptr[i] as usize, self.row_ptr[i + 1] as usize);
        match self.cols[s..e].binary_search(&j) {
            Ok(k) => s + k,
            Err(_) => panic!("entry ({i}, {j}) not in pattern"),
        }
    }

    #[inline]
    pub fn add(&mut self, i: usize, j: u32, b: &[f64; 9]) {
        let k = self.find(i, j);
        let v = &mut self.vals[k];
        for t in 0..9 {
            v[t] += b[t];
        }
    }

    /// y = A x.
    pub fn mul(&self, x: &[f64], y: &mut [f64]) {
        for i in 0..self.n {
            let (s, e) = (self.row_ptr[i] as usize, self.row_ptr[i + 1] as usize);
            let mut acc = [0.0; 3];
            for k in s..e {
                let j = self.cols[k] as usize * 3;
                let b = &self.vals[k];
                let (x0, x1, x2) = (x[j], x[j + 1], x[j + 2]);
                acc[0] += b[0] * x0 + b[1] * x1 + b[2] * x2;
                acc[1] += b[3] * x0 + b[4] * x1 + b[5] * x2;
                acc[2] += b[6] * x0 + b[7] * x1 + b[8] * x2;
            }
            y[3 * i] = acc[0];
            y[3 * i + 1] = acc[1];
            y[3 * i + 2] = acc[2];
        }
    }

    pub fn nnz_blocks(&self) -> usize {
        self.cols.len()
    }
}

/// Inverse of a 3×3 matrix (row-major).
pub fn inv3(m: &[f64; 9]) -> [f64; 9] {
    let [a, b, c, d, e, f, g, h, i] = *m;
    let co = [e * i - f * h, -(d * i - f * g), d * h - e * g];
    let det = a * co[0] + b * co[1] + c * co[2];
    let r = 1.0 / det;
    [
        co[0] * r,
        -(b * i - c * h) * r,
        (b * f - c * e) * r,
        co[1] * r,
        (a * i - c * g) * r,
        -(a * f - c * d) * r,
        co[2] * r,
        -(a * h - b * g) * r,
        (a * e - b * d) * r,
    ]
}

#[inline]
pub fn mv3(m: &[f64; 9], x: [f64; 3]) -> [f64; 3] {
    [
        m[0] * x[0] + m[1] * x[1] + m[2] * x[2],
        m[3] * x[0] + m[4] * x[1] + m[5] * x[2],
        m[6] * x[0] + m[7] * x[1] + m[8] * x[2],
    ]
}
