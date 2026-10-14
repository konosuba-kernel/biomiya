#!/usr/bin/env python3
"""Source counts and explicitly predicted RV64 layouts, not compiler evidence."""
import hashlib
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[1]


def source(path):
    data = path.read_bytes()
    lines = data.decode().splitlines()
    return dict(path=str(path.relative_to(ROOT)), bytes=len(data),
                physical_lines=len(lines),
                nonblank_noncomment_lines=sum(bool(line.split('//', 1)[0].strip()) for line in lines),
                sha256=hashlib.sha256(data).hexdigest())


def align(size, alignment):
    return (size+alignment-1)//alignment*alignment


def record(fields):
    size, alignment = 0, 1
    for field_size, field_alignment in fields:
        size = align(size, field_alignment)+field_size
        alignment = max(alignment, field_alignment)
    return align(size, alignment), alignment


def variant(payloads):
    alignment = max([4]+[p[1] for p in payloads])
    return align(align(4, alignment)+max([0]+[p[0] for p in payloads]), alignment), alignment


def report():
    word, index = (8, 8), (4, 4)
    expression = variant([word, record([index, index])])
    instruction = variant([record([index, word]), record([index]*3), index])
    plain_node = record([index]*3+[word])
    plain_instruction = record([index]*4+[word])
    models = {}
    for name, node, ir in [('typed', expression, instruction), ('tagged', plain_node, plain_instruction)]:
        models[name] = dict(node_size=node[0], instruction_size=ir[0],
                            node_array_bytes=17*node[0], instruction_array_bytes=18*ir[0],
                            value_array_bytes=17*word[0],
                            array_total_bytes=17*node[0]+18*ir[0]+17*word[0])
    files = ['typed.vt', 'tagged.vt', 'program.vt', 'tagged_program.vt', 'wrong_id.vt',
             'tagged_wrong_id.vt', 'private_id.vt', 'stale_id.vt', 'tagged_stale_id.vt']
    return dict(status='source-measured-layout-predicted-target-not-run',
                sources=[source(HERE/name) for name in files],
                io_sample=source(ROOT/'experiments/system/file_rollback.vt'),
                layout_predictions=models,
                excluded_from_array_totals='five metadata statics: four usize counters plus one bool; ELF alignment, code, stack and emitter costs not measured')


if __name__ == '__main__':
    print(json.dumps(report(), indent=2))
