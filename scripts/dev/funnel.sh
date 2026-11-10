#!/usr/bin/env bash
# The launch funnel without telemetry: one snapshot of GitHub's own numbers per run, appended
# to a local JSON-lines file. GitHub keeps traffic for 14 days only, so run it daily.
#
#   scripts/dev/funnel.sh            # record a snapshot, print one line
#   scripts/dev/funnel.sh summary    # per-day table from every snapshot recorded so far
#
# Reads: stars, forks, watchers; views and clones per day; top referrers and paths; release
# download counts. Needs `gh` (logged in with push access to the repository) and `jq`.
# The file stays on this machine (default ~/.local/share/markdownattractor/funnel.jsonl,
# override with MDA_FUNNEL_FILE); nothing is sent anywhere but GitHub's API.
set -euo pipefail

REPO="${MDA_FUNNEL_REPO:-joe-carr-data/markdownattractor}"
FILE="${MDA_FUNNEL_FILE:-$HOME/.local/share/markdownattractor/funnel.jsonl}"

for tool in gh jq; do
  command -v "$tool" >/dev/null 2>&1 || { echo "funnel: $tool not found" >&2; exit 1; }
done

summary() {
  [ -s "$FILE" ] || { echo "funnel: no snapshots yet in $FILE" >&2; exit 1; }
  # A day appears in up to 14 snapshots; the latest snapshot's figure for a day wins.
  # Stars and downloads are totals: the last snapshot taken on each day.
  jq -rs '
    (map(.views[]?) | group_by(.timestamp[:10]) | map({key: .[0].timestamp[:10], value: .[-1]}) | from_entries) as $views
    | (map(.clones[]?) | group_by(.timestamp[:10]) | map({key: .[0].timestamp[:10], value: .[-1]}) | from_entries) as $clones
    | (group_by(.at[:10]) | map({key: .[0].at[:10], value: .[-1]}) | from_entries) as $snap
    | (($views | keys) + ($clones | keys) + ($snap | keys) | unique) as $days
    | (["day", "views", "visitors", "clones", "cloners", "stars", "downloads"] | @tsv),
      ($days[] | [., ($views[.].count // "-"), ($views[.].uniques // "-"),
                  ($clones[.].count // "-"), ($clones[.].uniques // "-"),
                  ($snap[.].stars // "-"), ($snap[.].downloads_total // "-")] | @tsv)
  ' "$FILE" | column -t -s "$(printf '\t')"
  echo
  echo "top referrers (latest snapshot):"
  tail -n 1 "$FILE" | jq -r '.referrers[]? | "  \(.referrer)\t\(.count) views\t\(.uniques) visitors"'
}

record() {
  mkdir -p "$(dirname "$FILE")"
  repo="$(gh api "repos/$REPO")"
  views="$(gh api "repos/$REPO/traffic/views")"
  clones="$(gh api "repos/$REPO/traffic/clones")"
  referrers="$(gh api "repos/$REPO/traffic/popular/referrers")"
  paths="$(gh api "repos/$REPO/traffic/popular/paths")"
  releases="$(gh api --paginate "repos/$REPO/releases")"
  line="$(jq -cn \
    --arg at "$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
    --argjson repo "$repo" --argjson views "$views" --argjson clones "$clones" \
    --argjson referrers "$referrers" --argjson paths "$paths" --argjson releases "$releases" '
    ($releases | map({tag: .tag_name, downloads: ([.assets[] | select(.name != "SHA256SUMS") | .download_count] | add // 0)})) as $rel
    | {at: $at,
       stars: $repo.stargazers_count, forks: $repo.forks_count, watchers: $repo.subscribers_count,
       views: $views.views, clones: $clones.clones,
       referrers: $referrers,
       paths: ($paths | map({path, count, uniques})),
       releases: $rel, downloads_total: ($rel | map(.downloads) | add // 0)}')"
  printf '%s\n' "$line" >>"$FILE"
  printf '%s\n' "$line" | jq -r '"funnel: \(.stars) stars · \(.views | map(.count) | add // 0) views and \(.clones | map(.count) | add // 0) clones in 14 days · \(.downloads_total) downloads → '"$FILE"'"'
}

case "${1:-record}" in
  record) record ;;
  summary) summary ;;
  *) echo "usage: funnel.sh [record|summary]" >&2; exit 2 ;;
esac
