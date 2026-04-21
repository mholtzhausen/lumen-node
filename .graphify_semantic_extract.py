import json, os
from pathlib import Path
from graphify.extract import extract_document, extract_image

uncached = Path('.graphify_uncached.txt').read_text().splitlines()
results = {'nodes': [], 'edges': [], 'hyperedges': [], 'input_tokens': 0, 'output_tokens': 0}
for f in uncached:
    # sanitize potential line number prefix
    f = f.split('|', 1)[-1].strip()
    if f.endswith('.md') or f.endswith('.txt') or f.endswith('.html'):
        # treat as document
        doc_result = extract_document(Path(f))
        for k in ('nodes', 'edges', 'hyperedges'):
            results[k].extend(doc_result[k])
        results['input_tokens'] += doc_result.get('input_tokens', 0)
        results['output_tokens'] += doc_result.get('output_tokens', 0)
    elif f.endswith('.png') or f.endswith('.jpg') or f.endswith('.webp'):
        img_result = extract_image(Path(f))
        for k in ('nodes', 'edges', 'hyperedges'):
            results[k].extend(img_result[k])
        results['input_tokens'] += img_result.get('input_tokens', 0)
        results['output_tokens'] += img_result.get('output_tokens', 0)
    else:
        print(f'WARNING: {f} not recognized for semantic extraction')
with open('.graphify_semantic_new.json', 'w') as out:
    json.dump(results, out, indent=2)
print(f"Semantic extraction complete: {len(results['nodes'])} nodes, {len(results['edges'])} edges, {len(results['hyperedges'])} hyperedges")
