//! Lays commits out in lanes so the history can be drawn as a branch graph.

use crate::git::Commit;

#[derive(Clone, Default)]
pub struct Row {
    /// Lane of this commit's dot.
    pub lane: usize,
    /// Lanes coming in from above that end at this dot.
    pub incoming: Vec<usize>,
    /// Lanes leaving the dot downwards (to the parents).
    pub outgoing: Vec<usize>,
    /// Lanes passing straight through this row.
    pub through: Vec<usize>,
    /// Whether anything comes from above into the dot lane.
    pub has_above: bool,
}

pub fn layout(commits: &[Commit]) -> (Vec<Row>, usize) {
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::with_capacity(commits.len());
    let mut max = 1;
    for c in commits {
        let before = lanes.clone();
        let idx = match lanes.iter().position(|l| l.as_deref() == Some(c.sha.as_str())) {
            Some(i) => i,
            None => match lanes.iter().position(|l| l.is_none()) {
                Some(i) => i,
                None => {
                    lanes.push(None);
                    lanes.len() - 1
                }
            },
        };
        let incoming: Vec<usize> = before
            .iter()
            .enumerate()
            .filter(|(_, l)| l.as_deref() == Some(c.sha.as_str()))
            .map(|(i, _)| i)
            .collect();
        for &i in &incoming {
            lanes[i] = None;
        }
        lanes[idx] = c.parents.first().cloned();
        let mut outgoing = Vec::new();
        if !c.parents.is_empty() {
            outgoing.push(idx);
        }
        for p in c.parents.iter().skip(1) {
            if let Some(i) = lanes.iter().position(|l| l.as_deref() == Some(p.as_str())) {
                outgoing.push(i);
            } else if let Some(i) = lanes.iter().position(|l| l.is_none()) {
                lanes[i] = Some(p.clone());
                outgoing.push(i);
            } else {
                lanes.push(Some(p.clone()));
                outgoing.push(lanes.len() - 1);
            }
        }
        let through: Vec<usize> = before
            .iter()
            .enumerate()
            .filter(|(i, l)| l.is_some() && !incoming.contains(i) && lanes.get(*i).map(|x| x == *l).unwrap_or(false))
            .map(|(i, _)| i)
            .collect();
        while lanes.last().map(|l| l.is_none()).unwrap_or(false) {
            lanes.pop();
        }
        max = max.max(lanes.len()).max(idx + 1).max(before.len());
        rows.push(Row { lane: idx, has_above: !incoming.is_empty(), incoming, outgoing, through });
    }
    (rows, max)
}
