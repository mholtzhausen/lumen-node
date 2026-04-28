---
name: version-bump
description: Maintains project versioning and release notes with semantic version bumps (major, minor, patch, build). Use when the user asks to bump versions, cut a release, update changelog entries, or report current and next version from git history.
---

# Version Bump and Changelog

## Purpose

Keep a project's version consistent and produce release notes in `CHANGELOG.md` whenever a new version is created.

This skill:
- Detects current version from `Cargo.toml`
- Calculates next version by bump type (`major`, `minor`, `patch`, `build`)
- Reports both current and next version before editing files
- Creates or updates `CHANGELOG.md` using git history since the previous release
- When the repo uses them, keeps **`.xml` release metadata** (AppStream metainfo, `appdata.xml`, etc.) aligned with the new version so preflight/CI and stores stay consistent

## Inputs

Collect these before making changes:
- Bump type: `major` | `minor` | `patch` | `build`
- Version source file and format (e.g. `Cargo.toml` with `version = "x.y.z"`)

## Instructions

### 1. Detect the current version

Read the version from [`Cargo.toml`](Cargo.toml) (field `version = "x.y.z"`).

### 2. Calculate the next version

| Bump type | Example (1.2.1 → ?) |
|-----------|---------------------|
| `patch`   | 1.2.2               |
| `minor`   | 1.3.0               |
| `major`   | 2.0.0               |
| `build`   | 1.2.1-build.N       |

Report **both** the current and next version to the user before making any edits.

### 3. Retrieve git history since the previous release

Find the previous release's commit hash from the **topmost entry** in [`CHANGELOG.md`](CHANGELOG.md). Each entry has the format `## x.y.z (abbreviated_hash)`.

Then run:

```
git log <previous-hash>..HEAD --oneline --no-merges
```

For example:

```
git log 4fd15b6..HEAD --oneline --no-merges
```

### 4. Categorize changes from the log output

Read each commit message and classify it into one of three groups:

| Category     | Commit prefix convention                              |
|--------------|-------------------------------------------------------|
| Features     | `feat:`, `feature:`, or any functional addition       |
| Bugfixes     | `fix:`, `bugfix:`, `hotfix:`, or any defect correction |
| Deprecations | `deprecate:`, `remove:`, `chore:` (breaking removals) |

**General `chore:` commits** (e.g. version bumps, CI config, dependency updates) should typically be **omitted** from the changelog unless they represent a meaningful deprecation.

**Processing rules:**
- Strip the commit prefix and hash — rewrite as a concise, user-facing sentence.
- If a message already reads well (e.g. "add file open dialog"), keep it as-is.
- If a message is vague, rephrase it to be meaningful to an end-user.
- Deduplicate if multiple commits relate to the same change.
- If a category has no commits, **omit the section entirely** — do not write placeholder text like "None in this release."

### 5. Update `Cargo.toml`

Bump the `version` field to the new version.

### 6. Update `CHANGELOG.md`

Insert a new entry **at the top** of the file, above the existing entries, using this template:

```markdown
## x.y.z (abbreviated_hash)
### Bugfixes
- <categorized bugfix items>

### Features and Improvements
- <categorized feature items>

### Deprecations
- <categorized deprecation items>
```

**If any category has no entries, omit that section heading and its list entirely.** Do not include empty sections or placeholder text. For example, if there are no bugfixes and no deprecations, the entry would look like:

```markdown
## x.y.z (abbreviated_hash)
### Features and Improvements
- Added file open dialog.
- Added version-bump skill.
```

The `abbreviated_hash` is the **current** HEAD commit hash, obtained via:

```
git rev-parse --short HEAD
```

### 7. Update AppStream metainfo (if present)

If `data/com.lumennode.app.metainfo.xml` exists, add a new `<release>` entry inside the `<releases>` element:

```xml
<release version="x.y.z" date="<current-date-iso-8601>"/>
```

Use the current date in ISO 8601 format (YYYY-MM-DD).

### 8. Report summary

Present the user with a summary of:
- Version change (old → new)
- Files modified
- Number of changes categorized into each section
