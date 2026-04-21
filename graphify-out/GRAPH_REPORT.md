# Graph Report - /media/nemesarial/SmallData/code/lumen-node  (2026-04-21)

## Corpus Check
- 6 files · ~78,513 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 82 nodes · 174 edges · 13 communities detected
- Extraction: 83% EXTRACTED · 17% INFERRED · 0% AMBIGUOUS · INFERRED: 29 edges (avg confidence: 0.8)
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
- [[_COMMUNITY_Community 12|Community 12]]

## God Nodes (most connected - your core abstractions)
1. `build_ui()` - 25 edges
2. `open()` - 10 edges
3. `build_index_row()` - 9 edges
4. `scan_directory()` - 8 edges
5. `ensure_indexed()` - 7 edges
6. `hash_thumb_path()` - 6 edges
7. `hash_file()` - 5 edges
8. `emit_click_report()` - 5 edges
9. `try_finalize_click_trace()` - 5 edges
10. `load_metadata_async()` - 5 edges

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
Cohesion: 0.24
Nodes (16): build_index_row(), create_schema(), db_path(), ensure_indexed(), favourite_for_path(), file_mtime(), file_size(), get_cached() (+8 more)

### Community 1 - "Community 1"
Cohesion: 0.27
Nodes (10): apply_text_chunk(), DefaultMetadataDispatcher, extract_comfyui_prompts(), extract_comfyui_summary(), extract_exif(), extract_png(), extract_png_with_exif(), ImageMetadata (+2 more)

### Community 2 - "Community 2"
Cohesion: 0.42
Nodes (9): ensure_thumbnail(), file_uri(), generate_and_cache(), hash_cache_dir(), is_valid(), normal_cache_dir(), source_mtime(), thumb_path() (+1 more)

### Community 3 - "Community 3"
Cohesion: 0.33
Nodes (5): build_tree_root(), ClickStepTiming, get_mount_points(), PreviewLoadMetrics, PreviewLoadOutcome

### Community 4 - "Community 4"
Cohesion: 0.33
Nodes (4): attach_context_menu(), format_metadata_text(), FullViewTrace, load_picture_async()

### Community 5 - "Community 5"
Cohesion: 0.5
Nodes (4): load_metadata_async(), mark_click_step(), populate_metadata_sidebar(), try_finalize_click_trace()

### Community 6 - "Community 6"
Cohesion: 0.5
Nodes (5): build_ui(), extract_seed_from_parameters(), format_generation_command(), selected_image_path(), sync_tree_to_path()

### Community 7 - "Community 7"
Cohesion: 0.6
Nodes (4): AppConfig, config_path(), load(), save()

### Community 8 - "Community 8"
Cohesion: 0.6
Nodes (4): prune_missing(), is_image(), scan_directory(), sort_paths()

### Community 9 - "Community 9"
Cohesion: 0.67
Nodes (1): AtomicTaskGuard

### Community 10 - "Community 10"
Cohesion: 0.67
Nodes (3): emit_click_report(), emit_full_view_report(), write_timing_report()

### Community 11 - "Community 11"
Cohesion: 0.67
Nodes (1): ClickTrace

### Community 12 - "Community 12"
Cohesion: 1.0
Nodes (1): LumenNode UI - Image Gallery with Professional Context

## Knowledge Gaps
- **9 isolated node(s):** `ImageRow`, `ClickStepTiming`, `PreviewLoadOutcome`, `PreviewLoadMetrics`, `AppConfig` (+4 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **Thin community `Community 12`** (1 nodes): `LumenNode UI - Image Gallery with Professional Context`
  Too small to be a meaningful cluster - may be noise or needs more connections extracted.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **Why does `build_ui()` connect `Community 6` to `Community 0`, `Community 2`, `Community 3`, `Community 4`, `Community 5`, `Community 7`, `Community 8`, `Community 10`?**
  _High betweenness centrality (0.396) - this node is a cross-community bridge._
- **Why does `open()` connect `Community 0` to `Community 1`, `Community 2`, `Community 6`, `Community 8`, `Community 10`?**
  _High betweenness centrality (0.145) - this node is a cross-community bridge._
- **Why does `extract_png()` connect `Community 1` to `Community 0`, `Community 4`?**
  _High betweenness centrality (0.103) - this node is a cross-community bridge._
- **Are the 10 inferred relationships involving `build_ui()` (e.g. with `load()` and `scan_directory()`) actually correct?**
  _`build_ui()` has 10 INFERRED edges - model-reasoned connections that need verification._
- **Are the 6 inferred relationships involving `open()` (e.g. with `scan_directory()` and `build_ui()`) actually correct?**
  _`open()` has 6 INFERRED edges - model-reasoned connections that need verification._
- **Are the 2 inferred relationships involving `build_index_row()` (e.g. with `.extract()` and `generate_hash_thumbnail()`) actually correct?**
  _`build_index_row()` has 2 INFERRED edges - model-reasoned connections that need verification._
- **Are the 5 inferred relationships involving `scan_directory()` (e.g. with `.new()` and `open()`) actually correct?**
  _`scan_directory()` has 5 INFERRED edges - model-reasoned connections that need verification._