use super::*;

fn ts(s: i64) -> Timestamp {
    Timestamp::from_second(s).unwrap()
}

#[test]
fn authored_time_is_capped_at_committer_time_and_at_now() {
    let now = ts(2_000);
    assert_eq!(authored(100, 500, now), Some(ts(100)), "author before committer");
    assert_eq!(authored(900, 500, now), Some(ts(500)), "author after committer: capped");
    assert_eq!(authored(5_000, 6_000, now), Some(ts(2_000)), "both in the future: capped at now");
}

#[test]
fn line_count_counts_a_last_line_without_a_newline() {
    assert_eq!(line_count(b""), 0);
    assert_eq!(line_count(b"a\n"), 1);
    assert_eq!(line_count(b"a\nb"), 2);
    assert_eq!(line_count(b"a\n\n"), 2);
}

#[test]
fn line_porcelain_gives_one_time_per_line_in_order() {
    let id_a = "a".repeat(40);
    let id_b = "b".repeat(40);
    let out = format!(
        "{id_a} 1 1 1\nauthor A\nauthor-time 100\ncommitter-time 150\nsummary first\nfilename x.md\n\t# Title\n\
         {id_b} 2 2 1\nauthor B\nauthor-time 300\ncommitter-time 250\nprevious {id_a} x.md\nfilename x.md\n\tauthor-time 999 is text\n\
         {id_a} 2 3 1\nauthor A\nauthor-time 100\ncommitter-time 150\nfilename x.md\n\t\n"
    );
    let times = parse_line_porcelain(out.as_bytes(), ts(10_000)).unwrap();
    assert_eq!(times, vec![ts(100), ts(250), ts(100)], "tab-prefixed content is never a key");
}

#[test]
fn line_porcelain_rejects_lines_out_of_order_or_without_a_header() {
    let id = "c".repeat(40);
    let skipped =
        format!("{id} 1 2 1\nauthor-time 1\ncommitter-time 1\nfilename x\n\tsecond line first\n");
    assert!(parse_line_porcelain(skipped.as_bytes(), ts(10)).is_err());
    assert!(parse_line_porcelain(b"\torphan\n", ts(10)).is_err());
    assert!(parse_line_porcelain(b"", ts(10)).unwrap().is_empty());
}

#[test]
fn a_directory_outside_any_repository_is_not_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    // A ceiling is removed from the environment by `base`, so walk up from a fresh temp dir:
    // the system temp directory is not inside a work tree on any CI runner or dev machine.
    let found = discover(dir.path());
    assert!(matches!(found, Discovery::NotARepository | Discovery::NoGit), "{}", found.name());
}
