# Selective CI builds

## Goal

A change that touches only the web client must not build or test the API. Target: under 4 minutes for a web-only change by 2026-10-15.

## Design

The pipeline computes the affected packages from the git diff against `main` using the dependency graph in `packages/`. Jobs for unaffected packages are skipped; a nightly full build guards against graph mistakes.

## Rollout

Behind the `ci-selective` flag for two weeks, comparing skipped jobs against the full build. Owner: Lena.
