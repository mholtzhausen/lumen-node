## 1.6.0 (2893af0)
### Features and Improvements
- Added full-view favourite HUD with configurable icon and display duration.
- Added `color_scheme` config and a header theme toggle (system / light / dark).
- Improved folder tree navigation: single-click browses images; double-click sets the tree root.
- Restored selection after sort/filter and cleared preview/metadata when nothing is selected.
- Expanded README, ARCHITECTURE, and CLAUDE documentation (shortcuts, session/tree root, starter pack).

### Bugfixes
- No explicit bugfix-only commits were recorded in this release range.

### Deprecations
- No deprecations were introduced in this release.

## 1.5.0 (bfaf208)
### Features and Improvements
- Added favorites filter functionality to the UI.
- Added favourite support in image scanning and UI flows.
- Included additional incremental improvements from recent commit history.

### Bugfixes
- No explicit bugfix-only commits were recorded in this release range.

### Deprecations
- No deprecations were introduced in this release.

## 1.4.0 (1a378ea)
### Features and Improvements
- Improved folder indexing and refresh flow by streamlining lifecycle management internals.
- Enhanced database-backed image row handling and tightened UI interaction behavior.
- Refined selection handling and UI wiring to improve interaction consistency.
- Consolidated thumbnail-size and navigation-related refactors plus additional incremental improvements from recent commits.

### Bugfixes
- No explicit bugfix-only commits were recorded in this release range.

### Deprecations
- No deprecations were introduced in this release.

## 1.3.0 (8950049)
### Features and Improvements
- Implemented deferred thumbnail refresh and optimized image loading in grid view.
- Enhanced navigation and layout by adding footer bar and fullscreen toggle functionality.
- Added controls row to header and updated layout assembly for improved UI structure.
- Added version-bump skill for managing project versioning and changelog updates.

## 1.2.2 (f48e5b5)
### Features and Improvements
- Added file open dialog and enhanced update banner functionality.
- Added version-bump skill for managing project versioning and changelog updates.

## 1.2.1 (4fd15b6)
### Bugfixes
- Fixed missing drop of `bound_paths_map` in grid list item binding.

## 1.2.0 (e42d11d)
### Features and Improvements
- Integrated AppState into UI components and added test target in Makefile.
- Added safe storage for thumbnail generations and bound paths in AppState.

### Bugfixes
- Updated GitHub API URL in check_for_update function.
- Cleaned up unused variables and imports in UI modules.

## 1.1.0 (b293a81)
### Features and Improvements
- Enhanced build and runtime diagnostics via Makefile and runtime reporting updates.
- Added runtime environment reporting and a configuration dialog for improved visibility and control.

## 1.0.0 (baf519d)
### Features and Improvements
- Large-scale modularization: scan runtime and coordination, app state, layout, navigation, open-folder flow, tree and header controls, center grid, preview loading, metadata and JSON tree views, dialogs, and keyboard/UI wiring.
- Session persistence and restoration improvements for window and browsing state.
- Selection-driven metadata and preview loading, click tracing, and runtime snapshot support.
- Grid enhancements including scroll overlay and speed tuning, thumbnail rename/delete actions, and improved trash dialog and shortcuts.
- External editor support from the context menu and configuration.
- Supporting modules for recent folders, sort flags, thumbnail sizing, window layout math, image typing, filename helpers, and related utilities.

### Deprecations
- Removed handoff/resume shell command files in favor of in-app session persistence.

## 0.2.0 (c458607)
### Features and Improvements
- Added current folder path tracking in scan progress and status bar.
- Added recent folders support and persisted filter/sort/UI state behavior.
- Added thumbnail rename modal with live validation and improved grid interactions.
- Updated repository docs and architecture notes for recent configuration and UX changes.

### Bugfixes
- Fixed empty-folder DB cleanup and unified thumbnail card state handling.
- Fixed grid scroll/focus selection behavior updates.
- Fixed ignore rules for graphify temporary files and packaging artifacts.

### Deprecations
- Removed obsolete graphify scripts and old AppImage build script.
