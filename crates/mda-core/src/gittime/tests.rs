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

/// Is `dir` inside some git work tree (a TMPDIR below a checkout)? Then "not a repository"
/// cannot be tested there.
fn inside_a_work_tree(dir: &std::path::Path) -> bool {
    Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--is-inside-work-tree"])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
fn a_directory_outside_any_repository_is_not_a_repository() {
    let dir = tempfile::tempdir().unwrap();
    if inside_a_work_tree(dir.path()) {
        return;
    }
    let found = discover(dir.path());
    assert!(matches!(found, Discovery::NotARepository | Discovery::NoGit), "{}", found.name());
}

/// A shell command with the same pipe setup as a git call.
#[cfg(unix)]
fn sh(script: &str) -> Command {
    let mut cmd = Command::new("sh");
    cmd.args(["-c", script]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    cmd
}

#[cfg(unix)]
#[test]
fn a_call_that_outlives_its_timeout_is_killed_and_reported() {
    let started = Instant::now();
    let err = run_within(sh("sleep 30"), None, "sleep", Duration::from_millis(200), 1024);
    assert!(matches!(err, Err(Error::Git(ref m)) if m.contains("timed out")), "{:?}", err.err());
    assert!(started.elapsed() < Duration::from_secs(5), "bounded: {:?}", started.elapsed());
}

#[cfg(unix)]
#[test]
fn a_descendant_holding_the_pipes_cannot_hold_the_call_past_its_deadline() {
    // The shell exits at once; its background child keeps stdout and stderr open.
    let started = Instant::now();
    let err = run_within(sh("sleep 30 & exit 0"), None, "orphan", Duration::from_millis(300), 1024);
    assert!(matches!(err, Err(Error::Git(ref m)) if m.contains("timed out")), "{:?}", err.err());
    assert!(started.elapsed() < Duration::from_secs(5), "bounded: {:?}", started.elapsed());
}

#[cfg(unix)]
#[test]
fn a_call_that_passes_the_output_cap_is_killed_at_once() {
    let started = Instant::now();
    let err = run_within(sh("yes"), None, "yes", Duration::from_secs(20), 4096);
    assert!(matches!(err, Err(Error::Git(ref m)) if m.contains("more output")), "{:?}", err.err());
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "killed, not timed out: {:?}",
        started.elapsed()
    );
    // Too much on stderr counts too.
    let err = run_within(sh("yes 1>&2"), None, "yes", Duration::from_secs(20), 4096);
    assert!(matches!(err, Err(Error::Git(_))), "{:?}", err.err());
}

#[cfg(unix)]
#[test]
fn stdin_is_fed_while_output_is_read_and_a_normal_call_returns_its_output() {
    // More than a pipe buffer in each direction: a helper that wrote before reading would hang.
    let input = vec![b'x'; 512 * 1024];
    let out =
        run_within(sh("cat"), Some(input.clone()), "cat", Duration::from_secs(20), 1024 * 1024)
            .unwrap()
            .unwrap();
    assert!(out.success);
    assert_eq!(out.stdout.len(), input.len());
}
