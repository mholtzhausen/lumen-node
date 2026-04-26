## 1.1.0 (b293a81)
### Features and Improvements
- Enhanced build and runtime diagnostics via Makefile and runtime reporting updates.
- Added runtime environment reporting and a configuration dialog for improved visibility and control.

### Bugfixes
- None in this release.

### Deprecations
- None in this release.

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
