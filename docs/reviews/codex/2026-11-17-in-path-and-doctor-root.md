# Codex review — `--in` with absolute paths, `doctor` and the root (2026-11-17, PR #93)

Reviewer: Codex (`gpt-6-astra`, fresh thread `01a1eb77-946c-7aa0-9ba5-8fb7a4b4fc6a`, read-only, static review, through the `codex:codex-rescue` companion; no nested `codex exec`).

**Verdict:** "Changes requested." Six findings (4 P2, 2 P3).

| # | Sev | Finding | Triage |
|---|---|---|---|
| 1 | P2 | An absolute folder path lost its trailing separator, so `/project/docs/` became `docs` and matched `docs2/…`. | **Fixed.** A trailing separator is kept. A bare prefix (`docs`) still matches `docs2/…`, as it always did: the filter is a string prefix, now said in `--help` and the commands doc. Test with sibling folders. |
| 2 | P2 | A failed `canonicalize` mixed canonical and typed spellings: a deleted folder, or a root reached through a symlink, was "not under" the root. | **Fixed.** The core function decides on the text alone (`.` and `..` resolved textually, the Windows verbatim prefix ignored) against every spelling of the root it is given. The command line resolves symlinks in the part of the path that exists and tries the typed spelling too. Tests: a deleted folder on `timeline`, two spellings of the root. |
| 3 | P2 | Through MCP, the fallback let `/project/../outside/probe` behave differently depending on whether the target existed: an existence probe outside the root. | **Fixed.** `path_prefix_under` never touches the filesystem; `..` is resolved textually before containment is checked, so that input is "not under the root" whatever exists. MCP uses only this function. The command line still looks paths up (`exists`, `canonicalize`): there the user names their own files in their own shell, and nothing is read from them; recorded as the reason these two calls do not go through `safe_join`, which guards reads of indexed content. |
| 4 | P2 | Trimming changed valid names (` docs`) and turned a whitespace-only prefix into no filter. | **Fixed.** Nothing is trimmed; only the empty string, `.` and `./` mean the whole root. Test. |
| 5 | P3 | A path outside the root came back from MCP as an internal error. | **Fixed.** `invalid_params`, like a bad time argument. |
| 6 | P3 | The current-folder fallback was not in the help. | **Fixed.** In `--help` and `docs/design/commands.md`: the root wins when both exist; the fallback applies only when nothing is at that path below the root. The rule lives in `commands::in_prefix`, shared by `search` and `timeline`. |

Other points:
- The test named `doctor_and_start…` never ran `start`. **Renamed** to what it tests. The `start` note is one `println!` on a helper that the doctor test covers (`ancestor_note`); a test that starts a daemon from a subfolder is **deferred** (backlog).
- The root-as-filter assertion could not show the filter was off. **Fixed**: it now finds a document outside `docs/` that the `docs` filter excludes.
- `doctor --json`: the note is appended to the free-form `detail` of the `root` check. **Accepted as is** (no promise that `detail` is a path).
- `start` prints the note on a line of its own, before its normal output. **Accepted as is**: `start` already prints several lines, and the hook and the first-run script pass an explicit root, which suppresses the note.
- Not verified by either side: case-insensitive spellings, UNC paths. Windows CI runs the CLI test; the core unit tests use Unix absolute paths and run on Unix only.
