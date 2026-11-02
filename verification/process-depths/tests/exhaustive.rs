
use ftml_process_depths_verification::depth::{DepthItem, DepthList, process_depths};

fn env(name: &str, default: usize) -> usize {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

struct Placed {
    id: usize,
    nesting: usize,
    ltype: u8,
}

fn walk(
    list: &DepthList<u8, usize>,
    ltype: u8,
    nesting: usize,
    out: &mut Vec<Placed>,
    empty_list: &mut bool,
) {
    if list.is_empty() {
        *empty_list = true;
    }
    for node in list {
        match node {
            DepthItem::Item(id) => out.push(Placed { id: *id, nesting, ltype }),
            DepthItem::List(inner, sub) => walk(sub, *inner, nesting + 1, out, empty_list),
        }
    }
}

fn describe(rows: &[(usize, u8, usize)]) -> String {
    rows.iter()
        .map(|(d, t, _)| format!("(depth {d}, type {t})"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn check(rows: &[(usize, u8, usize)]) -> Option<String> {
    let trees = process_depths(9u8, rows.to_vec());
    let mut placed = Vec::new();
    let mut empty_list = false;
    for (ltype, tree) in &trees {
        if tree.is_empty() {
            empty_list = true;
        }
        walk(tree, *ltype, 0, &mut placed, &mut empty_list);
    }
    let input = describe(rows);
    if placed.len() != rows.len() {
        return Some(format!("rows [{input}]: {} rows came out", placed.len()));
    }
    for (i, p) in placed.iter().enumerate() {
        let (depth, ltype, id) = rows[i];
        if p.id != id {
            return Some(format!("rows [{input}]: output position {i} holds row {} not row {id}", p.id));
        }
        if p.nesting != depth {
            return Some(format!("rows [{input}]: row {id} came out at nesting depth {} not {depth}", p.nesting));
        }
        if p.ltype != ltype {
            return Some(format!("rows [{input}]: row {id} sits in a list of type {} not {ltype}", p.ltype));
        }
    }
    if empty_list {
        return Some(format!("rows [{input}]: output contains an empty list"));
    }
    None
}

#[test]
fn every_input_inside_the_bounds_keeps_its_rows() {
    let max_rows = env("FTML_DEPTH_ROWS", 6);
    let max_depth = env("FTML_DEPTH_MAX", 3);
    let ltypes = env("FTML_DEPTH_TYPES", 3) as u8;

    let choices: Vec<(usize, u8)> = (0..=max_depth)
        .flat_map(|d| (0..ltypes).map(move |t| (d, t)))
        .collect();
    let mut checked = 0u64;
    let mut failures: Vec<String> = Vec::new();

    for len in 0..=max_rows {
        let mut pick = vec![0usize; len];
        loop {
            let rows: Vec<(usize, u8, usize)> = pick
                .iter()
                .enumerate()
                .map(|(id, &c)| (choices[c].0, choices[c].1, id))
                .collect();
            checked += 1;
            if let Some(message) = check(&rows) {
                if failures.len() < 5 {
                    failures.push(message);
                }
            }
            let mut pos = 0;
            while pos < len {
                pick[pos] += 1;
                if pick[pos] < choices.len() {
                    break;
                }
                pick[pos] = 0;
                pos += 1;
            }
            if pos == len {
                break;
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} inputs checked; first failures:\n{}",
        checked,
        failures.join("\n")
    );
    eprintln!("checked {checked} inputs, all properties held");
}
