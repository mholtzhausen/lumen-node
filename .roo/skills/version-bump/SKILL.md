---
name: version-bump
description: ---
name: version-bump
description: Maintains project versioning and release notes with semantic version bumps (major, minor, patch, build). Use when the user asks to bump versions, cut a release, update changelog entries, or report current and next version from git history.
---

# Version Bump and Changelog

## Purpose

Keep a project's version consistent and produce release notes in `CHANGELOG.md` whenever a new version is created.

This skill:
- Detects current version
- Calculates next version by bump type (`major`, `minor`, `patch`, `build`)
- Reports both current and next version before editing files
- Creates or updates `CHANGELOG.md` using git history since the previous release
- When the repo uses them, keeps **`.xml` release metadata** (AppStream metainfo, `appdata.xml`, etc.) aligned with the new version so preflight/CI and stores stay consistent

## Inputs

Collect these before making changes:
- Bump type: `major` | `minor` | `patch` | `build`
- Version source file and format (for example: `Cargo.
---

# Version Bump

## Instructions

Add your skill instructions here.
