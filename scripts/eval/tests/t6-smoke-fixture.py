#!/usr/bin/env python3
"""A one-question private fixture for t6-smoke.sh: <full.jsonl> <sample.jsonl> <evidence text>."""
import hashlib, json, sys
full, sample, ev = sys.argv[1:4]
h = lambda t: hashlib.sha256(t.encode()).hexdigest()
r = {"id": "private-c001", "q": "What happens on line two?", "reference": "A date is given. A code span is shown.", "evidence": ev, "evidence_sha256": h(ev),
     "required_facts": ["A date is given.", "A code span is shown."], "sources": ["x#y"], "relevant": ["x.md"]}
open(full, "w").write(json.dumps(r) + "\n")
open(sample, "w").write(json.dumps({"id": r["id"], "split": "own", "category": "single-section", "question_sha256": h(r["q"]), "reference_sha256": h(r["reference"]),
                                     "evidence_sha256": h(ev), "required_facts_n": 2}) + "\n")
