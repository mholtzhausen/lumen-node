import json, os
from graphify.cache import check_semantic_cache
from pathlib import Path

detect = json.loads(Path('.graphify_incremental.json').read_text()) if Path('.graphify_incremental.json').exists() else {}
all_files = [f for ft in ['document', 'paper', 'image'] for f in detect.get('new_files', {}).get(ft, [])]
cached_nodes, cached_edges, cached_hyperedges, uncached = check_semantic_cache(all_files)
if cached_nodes or cached_edges or cached_hyperedges:
    Path('.graphify_cached.json').write_text(json.dumps({'nodes': cached_nodes, 'edges': cached_edges, 'hyperedges': cached_hyperedges}))
Path('.graphify_uncached.txt').write_text('\n'.join(uncached))
print(f'Cache: {len(all_files)-len(uncached)} files hit, {len(uncached)} files need extraction')
