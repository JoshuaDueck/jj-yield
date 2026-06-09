//! Parser tests against fixtures captured from real `jj file show
//! --config ui.conflict-marker-style=snapshot` output (jj 0.42).

use jj_yield::conflict::{Accept, Segment, TermKind};
use jj_yield::parser::parse;

fn fixture(name: &str) -> String {
    let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("reading {path}: {e}"))
}

/// Every fixture must round-trip exactly when no resolution is chosen.
#[test]
fn round_trips_losslessly() {
    for name in ["two_sided.txt", "three_sided.txt", "multi_region.txt", "deletion.txt"] {
        let text = fixture(name);
        let parsed = parse(&text);
        let resolutions = vec![None; parsed.region_count()];
        assert_eq!(parsed.render(&resolutions), text, "round-trip failed for {name}");
    }
}

#[test]
fn two_sided() {
    let parsed = parse(&fixture("two_sided.txt"));
    assert_eq!(parsed.region_count(), 1);

    let region = parsed.region(0).unwrap();
    assert_eq!(region.sides(), 2);
    // terms: +Alice, -base, +Bob
    assert_eq!(region.terms.len(), 3);
    assert_eq!(region.terms[0].kind, TermKind::Add);
    assert_eq!(region.terms[1].kind, TermKind::Remove);
    assert_eq!(region.terms[2].kind, TermKind::Add);

    assert_eq!(region.terms[0].label.description, "Alice: rename var");
    assert_eq!(region.terms[2].label.description, "Bob: inline call");
    assert_eq!(region.terms[0].content, vec!["AAA".to_string()]);
    assert_eq!(region.terms[1].content, vec!["shared".to_string()]);

    // columns: Alice, base, Bob (one base)
    let cols = region.columns();
    assert_eq!(cols.len(), 3);
    assert_eq!(cols[0].kind, TermKind::Add);
    assert_eq!(cols[1].kind, TermKind::Remove);
    assert_eq!(cols[2].kind, TermKind::Add);
}

#[test]
fn three_sided_collapses_repeated_base() {
    let parsed = parse(&fixture("three_sided.txt"));
    let region = parsed.region(0).unwrap();
    assert_eq!(region.sides(), 3);
    // terms: +A, -base, +B, -base, +C  (base appears twice)
    assert_eq!(region.terms.len(), 5);
    // columns collapse the identical base -> A, base, B, C
    let cols = region.columns();
    assert_eq!(cols.len(), 4);
    assert_eq!(cols.iter().filter(|c| c.kind == TermKind::Remove).count(), 1);
    let descs: Vec<&str> = cols.iter().map(|c| c.label.description.as_str()).collect();
    assert_eq!(descs, vec!["Alice", "base", "Bob", "Carol"]);
}

#[test]
fn multi_region_with_context() {
    let parsed = parse(&fixture("multi_region.txt"));
    assert_eq!(parsed.region_count(), 2);

    // Headers carry "conflict n of m".
    assert_eq!(parsed.region(0).unwrap().header, Some((1, 2)));
    assert_eq!(parsed.region(1).unwrap().header, Some((2, 2)));

    // Context lines a / c / e are preserved between/around regions.
    let context: Vec<String> = parsed
        .segments
        .iter()
        .filter_map(|s| match s {
            Segment::Context(lines) => Some(lines.clone()),
            Segment::Conflict(_) => None,
        })
        .flatten()
        .collect();
    assert_eq!(context, vec!["a", "c", "e"]);
}

#[test]
fn deletion_side_has_empty_content() {
    let parsed = parse(&fixture("deletion.txt"));
    let region = parsed.region(0).unwrap();
    // +Alice (deletes -> empty), -base (X, Y), +Bob (XX, YY)
    assert_eq!(region.terms[0].kind, TermKind::Add);
    assert!(region.terms[0].content.is_empty(), "deleting side should be empty");
    assert_eq!(region.terms[1].content, vec!["X".to_string(), "Y".to_string()]);
    assert_eq!(region.terms[2].content, vec!["XX".to_string(), "YY".to_string()]);
}

#[test]
fn picking_a_side_replaces_the_region() {
    let text = fixture("two_sided.txt");
    let parsed = parse(&text);
    // Resolve region 0 to column 0 (Alice => "AAA").
    let rendered = parsed.render(&[Some(Accept::Side(0))]);
    assert_eq!(rendered, "line1\nAAA\nline3\n");
    assert!(!rendered.contains("<<<<<<<"));
    assert!(!rendered.contains("+++++++"));
}

#[test]
fn picking_base_in_three_sided() {
    let parsed = parse(&fixture("three_sided.txt"));
    // Column 1 is the base ("shared").
    let rendered = parsed.render(&[Some(Accept::Side(1))]);
    assert_eq!(rendered, "line1\nshared\nline3\n");
}

#[test]
fn accept_both_concatenates_in_order() {
    let parsed = parse(&fixture("two_sided.txt"));
    // Columns: 0 = Alice ("AAA"), 1 = base ("shared"), 2 = Bob ("BBB").
    let rendered = parsed.render(&[Some(Accept::Both(vec![0, 2]))]);
    assert_eq!(rendered, "line1\nAAA\nBBB\nline3\n");

    // Reversed order is honored.
    let rendered = parsed.render(&[Some(Accept::Both(vec![2, 0]))]);
    assert_eq!(rendered, "line1\nBBB\nAAA\nline3\n");
}
