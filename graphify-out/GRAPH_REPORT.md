# Graph Report - /media/nemesarial/SmallData/code/lumen-node  (2026-04-21)

## Corpus Check
- 6 files · ~79,946 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 91 nodes · 203 edges · 12 communities detected
- Extraction: 83% EXTRACTED · 17% INFERRED · 0% AMBIGUOUS · INFERRED: 35 edges (avg confidence: 0.8)
- Token cost: 0 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_Community 0|Community 0]]
- [[_COMMUNITY_Community 1|Community 1]]
- [[_COMMUNITY_Community 2|Community 2]]
- [[_COMMUNITY_Community 3|Community 3]]
- [[_COMMUNITY_Community 4|Community 4]]
- [[_COMMUNITY_Community 5|Community 5]]
- [[_COMMUNITY_Community 6|Community 6]]
- [[_COMMUNITY_Community 7|Community 7]]
- [[_COMMUNITY_Community 8|Community 8]]
- [[_COMMUNITY_Community 9|Community 9]]
- [[_COMMUNITY_Community 10|Community 10]]
- [[_COMMUNITY_Community 11|Community 11]]

## God Nodes (most connected - your core abstractions)
1. `build_ui()` - 31 edges
2. `open()` - 10 edges
3. `build_index_row()` - 9 edges
4. `load_grid_thumbnail()` - 9 edges
5. `scan_directory()` - 8 edges
6. `ensure_indexed()` - 7 edges
7. `hash_file()` - 6 edges
8. `ensure_thumbnail()` - 6 edges
9. `hash_thumb_path()` - 6 edges
10. `emit_click_report()` - 5 edges

## Surprising Connections (you probably didn't know these)
- `scan_directory()` --calls--> `open()`  [INFERRED]
  /media/nemesarial/SmallData/code/lumen-node/src/scanner.rs → /media/nemesarial/SmallData/code/lumen-node/src/db.rs
- `scan_directory()` --calls--> `ensure_indexed()`  [INFERRED]
  /media/nemesarial/SmallData/code/lumen-node/src/scanner.rs → /media/nemesarial/SmallData/code/lumen-node/src/db.rs
- `scan_directory()` --calls--> `build_ui()`  [INFERRED]
  /media/nemesarial/SmallData/code/lumen-node/src/scanner.rs → /media/nemesarial/SmallData/code/lumen-node/src/main.rs
- `open()` --calls--> `build_ui()`  [INFERRED]
  /media/nemesarial/SmallData/code/lumen-node/src/db.rs → /media/nemesarial/SmallData/code/lumen-node/src/main.rs
- `open()` --calls--> `extract_exif()`  [INFERRED]
  /media/nemesarial/SmallData/code/lumen-node/src/db.rs → /media/nemesarial/SmallData/code/lumen-node/src/metadata.rs

## Hyperedges (group relationships)
- **Message-Driven Data Flow Pipeline** — scanner_module, db_module, thumbnails_module, main_module [EXTRACTED 1.00]
- **Format-Specific Metadata Extraction** — metadatadispatcher_trait, defaultmetadatadispatcher, exif_extraction, png_text_extraction, comfyui_prompt_parsing [EXTRACTED 0.95]
- **Caching with Staleness Validation** — db_caching_staleness_check, db_module, thumbnails_module, hash_based_thumbnail_storage [INFERRED 0.82]

## Communities

### Community 0 - "Community 0"
Cohesion: 0.26
Nodes (16): load_grid_thumbnail(), refresh_realized_grid_thumbnails(), ensure_thumbnail(), file_uri(), generate_and_cache(), generate_hash_thumbnail_for_size(), hash_cache_dir(), hash_thumb_if_exists_for_size() (+8 more)

### Community 1 - "Community 1"
Cohesion: 0.28
Nodes (14): build_index_row(), create_schema(), db_path(), ensure_indexed(), favourite_for_path(), file_mtime(), file_size(), get_cached() (+6 more)

### Community 2 - "Community 2"
Cohesion: 0.27
Nodes (10): apply_text_chunk(), DefaultMetadataDispatcher, extract_comfyui_prompts(), extract_comfyui_summary(), extract_exif(), extract_png(), extract_png_with_exif(), ImageMetadata (+2 more)

### Community 3 - "Community 3"
Cohesion: 0.24
Nodes (8): build_tree_root(), ClickStepTiming, emit_full_view_report(), get_mount_points(), PreviewLoadMetrics, PreviewLoadOutcome, SortFields, write_timing_report()

### Community 4 - "Community 4"
Cohesion: 0.38
Nodes (7): build_ui(), extract_seed_from_parameters(), format_generation_command(), normalize_thumbnail_size(), selected_image_path(), sync_tree_to_path(), thumbnail_size_options()

### Community 5 - "Community 5"
Cohesion: 0.29
Nodes (5): attach_context_menu(), compute_sort_fields(), format_metadata_text(), FullViewTrace, load_picture_async()

### Community 6 - "Community 6"
Cohesion: 0.4
Nodes (5): emit_click_report(), load_metadata_async(), mark_click_step(), populate_metadata_sidebar(), try_finalize_click_trace()

### Community 7 - "Community 7"
Cohesion: 0.6
Nodes (4): prune_missing(), is_image(), scan_directory(), sort_paths()

### Community 8 - "Community 8"
Cohesion: 0.6
Nodes (4): AppConfig, config_path(), load(), save()

### Community 9 - "Community 9"
Cohesion: 0.67
Nodes (1): AtomicTaskGuard

### Community 10 - "Community 10"
Cohesion: 0.67
Nodes (1): ClickTrace

### Community 11 - "Community 11"
Cohesion: 1.0
Nodes (1): LumenNode UI - Image Gallery with Professional Context

## Knowledge Gaps
- **10 isolated node(s):** `ImageRow`, `ClickStepTiming`, `PreviewLoadOutcome`, `SortFields`, `PreviewLoadMetrics` (+5 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Community 11`** (1 nodes): `LumenNode UI - Image Gallery with Professional Context`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `build_ui()` connect `Community 4` to `Community 0`, `Community 1`, `Community 3`, `Community 5`, `Community 6`, `Community 7`, `Community 8`?**
  _High betweenness centrality (0.385) - this node is a cross-community bridge._
- **Why does `open()` connect `Community 1` to `Community 0`, `Community 2`, `Community 3`, `Community 4`, `Community 7`?**
  _High betweenness centrality (0.125) - this node is a cross-community bridge._
- **Why does `extract_png()` connect `Community 2` to `Community 1`, `Community 5`?**
  _High betweenness centrality (0.095) - this node is a cross-community bridge._
- **Are the 11 inferred relationships involving `build_ui()` (e.g. with `load()` and `scan_directory()`) actually correct?**
  _`build_ui()` has 11 INFERRED edges - model-reasoned connections that need verification._
- **Are the 6 inferred relationships involving `open()` (e.g. with `scan_directory()` and `build_ui()`) actually correct?**
  _`open()` has 6 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `build_index_row()` (e.g. with `.extract()` and `generate_hash_thumbnail()`) actually correct?**
  _`build_index_row()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 5 inferred relationships involving `load_grid_thumbnail()` (e.g. with `load()` and `hash_thumb_if_exists_for_size()`) actually correct?**
  _`load_grid_thumbnail()` has 5 INFERRED edges - model-reasoned connections that need verification._