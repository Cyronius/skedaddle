#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placed {
    pub idx: usize,
    pub col: usize,
    pub cols: usize,
}

/// `spans` are (start, end) minute offsets; returns one entry per span, in input order.
pub fn columns(spans: &[(i64, i64)]) -> Vec<Placed> {
    let mut order: Vec<usize> = (0..spans.len()).collect();
    order.sort_by_key(|&i| (spans[i].0, -(spans[i].1 - spans[i].0)));

    let mut placed = vec![Placed { idx: 0, col: 0, cols: 1 }; spans.len()];
    let mut cluster: Vec<usize> = Vec::new();
    let mut col_ends: Vec<i64> = Vec::new();
    let mut cluster_end = i64::MIN;

    let flush = |cluster: &mut Vec<usize>, col_ends: &mut Vec<i64>, placed: &mut Vec<Placed>| {
        let cols = col_ends.len().max(1);
        for &i in cluster.iter() {
            placed[i].cols = cols;
        }
        cluster.clear();
        col_ends.clear();
    };

    for &i in &order {
        let (s, e) = spans[i];
        if !cluster.is_empty() && s >= cluster_end {
            flush(&mut cluster, &mut col_ends, &mut placed);
            cluster_end = i64::MIN;
        }
        let col = match col_ends.iter().position(|&ce| ce <= s) {
            Some(c) => {
                col_ends[c] = e;
                c
            }
            None => {
                col_ends.push(e);
                col_ends.len() - 1
            }
        };
        placed[i] = Placed { idx: i, col, cols: 1 };
        cluster.push(i);
        cluster_end = cluster_end.max(e);
    }
    flush(&mut cluster, &mut col_ends, &mut placed);
    placed
}
